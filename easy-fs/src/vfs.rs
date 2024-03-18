use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use log::info;
use log::error;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
/// Virtual filesystem layer over easy-fs
pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// Create a vfs inode
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }
    /// Call a function over a disk inode to read it
    fn  read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count: usize = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some(dirent.inode_id() as u32);
            }
        }
        None
    }
    fn remove_dirent(&self, name:&str, disk_inode:&mut DiskInode) -> u32
    {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        // assert diskinode is empty
        assert!(disk_inode.size > 0);
        // size 
        assert!(disk_inode.size as usize%DIRENT_SZ == 0);
        let file_count = (disk_inode.size as usize)/DIRENT_SZ -1;
        let mut dirent = DirEntry::empty();
        let mut last_dirent = DirEntry::empty();

        info!("before read last dirent({} as_byte_mute{:?}", file_count , last_dirent.as_bytes_mut());
        assert_eq!(
            disk_inode.read_at((disk_inode.size as usize - DIRENT_SZ) as usize , 
            last_dirent.as_bytes_mut(),
            &self.block_device),
            DIRENT_SZ
        );
        info!("after read{} last dirent({} as_byte_mute{:?}", dirent.name(), file_count, last_dirent.as_bytes_mut());
        //file_count -=1;
        for i in 0..file_count{
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device),
                DIRENT_SZ
            );
            info!("read {} dirent:{}", &dirent.name(), i);
            if dirent.name() == name {
                disk_inode.write_at(DIRENT_SZ * i, &last_dirent.as_bytes_mut(), &self.block_device);
                info!("write dirent({} as_byte_mute{:?}", i, dirent.as_bytes_mut());
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device);
                info!("read dirent({} as_byte_mute{:?}", i, dirent.as_bytes_mut());

            }
        }
        disk_inode.write_at(DIRENT_SZ * file_count, &DirEntry::empty().as_bytes_mut(), &self.block_device);
        0
    }
    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs: MutexGuard<'_, EasyFileSystem> = self.fs.lock();
        self.read_disk_inode(|disk_inode: &DiskInode| {
            self.find_inode_id(name, disk_inode).map(|inode_id| {
                info!(" find {}'s inode_id:{} ",name, inode_id);
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                info!(" name:{} block_id{},offset({:?})", name, block_id, block_offset);
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }
    /// Find inode id for VFS
    pub fn get_inode_id(&self) -> Option<u64> {
        let fs = self.fs.lock();
        Some(fs.get_disk_inode_id(self.block_id as u32,self.block_offset) as u64)
    }
    /// check inode is a dir for VFS
    pub fn is_dir(&self) -> bool {
        self.read_disk_inode(|disk_inode: &DiskInode| {
            disk_inode.is_dir()
        })
    }
    /// check inode is a file for VFS
    pub fn is_file(&self) -> bool {
        self.read_disk_inode(|disk_inode: &DiskInode| {
            disk_inode.is_file()
        })
    }
    /// check inode links for VFS
    pub fn link_nums(&self) -> u32 {
        self.read_disk_inode(|disk_inode: &DiskInode| {
            disk_inode.linknum()
        })
    }
    /// decrease the size of a disk inode
    fn decrease_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size > disk_inode.size {
            return;
        }
        let data_blocks_dealloc = disk_inode.decrease_size(new_size, &self.block_device);
        for data_block in data_blocks_dealloc.into_iter() {
            fs.dealloc_data(data_block);
        }
    }
    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }
    /// new linke new name file with name file
    pub fn link(&self, src_inode: Arc<Inode>, new_name:&str) -> Option<Arc<Inode>> {
        info!(" try fs lock block_id{},offset({:?})", src_inode.block_id, src_inode.block_offset);
        let src_inode_id= src_inode.get_inode_id().unwrap();
        let mut fs = self.fs.lock();
        let (block_id, block_offset) = fs.get_disk_inode_pos(src_inode_id as u32);
        info!(" source linkefile inode_id {} ",src_inode_id);
        self.modify_disk_inode(|root_inode|{
            if self.find_inode_id(new_name, root_inode).is_some() {
                log::error!(" found new linkefile {} had exist",new_name);
                return None;
            }
            /* 增加inodedisk中link的计数 */
            get_block_cache(block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(block_offset, |new_inode: &mut DiskInode| {
                info!("{}'s nlinks{}", src_inode_id, new_inode.nlinks);
                new_inode.linkup();
                info!("after {}'s nlinks{}", src_inode_id, new_inode.nlinks);
            });
            info!(" save linkfile into dirrent ");
            /* 保存linkfile 到当前目录dir的dirent */
           
            // append file in the dirent
            let file_count: usize = (root_inode.size as usize) / DIRENT_SZ;
            let new_size: usize = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent: DirEntry = DirEntry::new(new_name, src_inode_id as u32);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
            //block_cache_sync_all();
            // return inode
            Some(Arc::new(Self::new(
                block_id,
                block_offset,
                self.fs.clone(),
                self.block_device.clone(),
            )))                

    
        })
    }
    /// unlink name file
    pub fn unlink(&self, src_inode:Arc<Inode>, name: &str) -> Option<isize> {
        let src_inode_id = src_inode.get_inode_id().unwrap();
        let mut fs = self.fs.lock();
        let (block_id, block_offset) = fs.get_disk_inode_pos(src_inode_id as u32);
        let linkcount = get_block_cache(block_id as usize, Arc::clone(&self.block_device))
        .lock()
        .modify(block_offset, |new_inode: &mut DiskInode| {
            new_inode.linkdown()
        });
        if linkcount == 0 {
            info!(" found new {:?} is a last files we need remove inode using del/rm operation",name);
            src_inode.modify_disk_inode(|disk_inode| {
                let size = disk_inode.size;
                let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
                assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
                for data_block in data_blocks_dealloc.into_iter() {
                    fs.dealloc_data(data_block);
                }
            });
        }
        self.modify_disk_inode(|root_inode: &mut DiskInode|{
            // delete file in the dirent
            let file_count: usize = (root_inode.size as usize) / DIRENT_SZ - 1;
            let new_size: usize = file_count * DIRENT_SZ;
            if self.remove_dirent(name, root_inode)  != 0 {
                return None;
            }
            info!("{}decrease_size {}->{}", name, root_inode.size, new_size);
            // decrease size
            self.decrease_size(new_size as u32, root_inode, &mut fs);
            Some(0)
        })
    }

    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
            });
        info!("{}'s inode_id:{}", name, new_inode_id);
        self.modify_disk_inode(|root_inode: &mut DiskInode| {
            // append file in the dirent
            let file_count: usize = (root_inode.size as usize) / DIRENT_SZ;
            let new_size: usize = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent: DirEntry = DirEntry::new(name, new_inode_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
        // release efs lock automatically by compiler
    }
    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                v.push(String::from(dirent.name()));
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }
}
