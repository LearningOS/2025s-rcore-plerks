use crate::BLOCK_SZ;

use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};

pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// We should not acquire efs lock here.
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

    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }

    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }
    
    /// Find inode under a disk inode by name
    /// 被find()调用
    pub fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some(dirent.inode_id() as u32);
            }
        }
        None
    }
    /// Find inode under current inode by name
    /// find方法只会被根目录Inode调用
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|inode_id| { // self.find_inode_id的self是Inode
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }

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
    /// Create inode under current inode by name
    /// <https://learningos.cn/rCore-Camp-Guide-2025S/chapter6/2fs-implementation-2.html#id5>:
    /// create方法可以在根目录下创建一个文件，该方法只有根目录的Inode会调用
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        let op = |root_inode: &mut DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.modify_disk_inode(op).is_some() {
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
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(name, new_inode_id);
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

    /// 实现硬链接，在根目录的内容中增加DirEntry
    pub fn add_dir_entry(&self, old_name: &str, new_name: &str) -> Result<(), ()> {
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            self.find_inode_id(old_name, root_inode)
        };
        let result = self.read_disk_inode(op);
        if result.is_none() { // 原文件不存在
            return Err(());
        }
        
        let inode_id = result.unwrap();

        // self是root inode，对根目录文件增加DirEntry
        self.modify_disk_inode(|root_inode| {
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            self.increase_size(new_size as u32, root_inode, &mut fs);
            let dirent = DirEntry::new(new_name, inode_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        drop(fs); // 这里要drop一下，不然下面find()里要fs.lock()，会死锁
        let target_file_disk_inode = self.find(old_name);
        target_file_disk_inode.unwrap().modify_disk_inode(|disk_inode| {
            disk_inode.nlink += 1; // 文件diskInode的引用计数+1
        });
        // panic!("here");

        Ok(())
    }

    /// 删除硬链接，在根目录的内容中删除DirEntry
    pub fn remove_dir_entry(&self, name: &str) -> Result<(), ()> {
        let mut fs = self.fs.lock();
        // modify_disk_inode()和read_disk_inode()会获取全局的那个BlockCache的锁(BLOCK_CACHE_MANAGER)，注意不要把二者套起来，比如modify_disk_inode里有和read_disk_inode，否则会死锁
        let (file_count, target_pos, target_inode_id) = self.modify_disk_inode(|root_disk_inode| {
            let file_count = (root_disk_inode.size as usize) / DIRENT_SZ;
            let mut target_pos = None; // 目标dirEntry的位置
            let mut target_inode_id = 0; // 目标dirEntry的inode号

            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                root_disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device);
                if dirent.name() == name {
                    target_pos = Some(i);
                    target_inode_id = dirent.inode_id();
                }
            }
            
            (file_count, target_pos, target_inode_id)
        });

        if target_pos.is_none() { // 文件没找到
            return Err(());
        }

        // 如果目标文件的引用计数为0，删除文件。target_inode为目标文件Inode，就是要unlink的那个文件
        /* let target_inode = self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|inode_id| { // self.find_inode_id的self是Inode
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        }).unwrap(); */
        let (block_id, block_offset) = fs.get_disk_inode_pos(target_inode_id);
        let target_inode = Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        ));
        let nlink = target_inode.modify_disk_inode(|disk_inode| {
            disk_inode.nlink -= 1;
            // block_cache_sync_all();
            disk_inode.nlink
        });
        if nlink == 0 {
            drop(fs);
            /* 用Inode.clear()删除数据块，clear()里要fs.lock()，所以这里要先drop(fs)。
            这几处drop(fs)，或许就是锁为什么要实现可重入功能？如果fs是可重入锁，就不用处理这个了。
            */
            target_inode.clear();
            fs = self.fs.lock();
            fs.dealloc_inode(target_inode_id as usize); // 删除inode
        }
        
        /*
        注意以下这样写会死锁，虽然处理了Inode.clear()里要获取fs的问题，但是
        Inode.clear()里要调用modify_disk_inode，而read_disk_inode和modify_disk_inode
        都要获取全局的那个BlockCache的锁(BLOCK_CACHE_MANAGER)，这会导致死锁。
        
        总结一下，实现link和unlink容易写出的两种死锁(至少我写出来了，而且不容易发现)：
        1. self.fs重复lock
        2. modify_disk_inode和read_disk_inode的嵌套调用，导致重复lock全局的BlockCache

        target_inode.read_disk_inode(|disk_inode| {
            if disk_inode.nlink == 0 {
                drop(fs);
                // panic!("here");
                target_inode.clear();
                // panic!("here2");
                fs = self.fs.lock();
                fs.dealloc_inode(target_inode_id as usize);
            }
        });
        */

        let target_index = target_pos.unwrap();
        self.modify_disk_inode(|root_disk_inode| {
            // 后面的前移，删除DirEntry
            for i in target_index + 1..file_count {
                let mut dirent = DirEntry::empty();
                root_disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device);
                root_disk_inode.write_at((i - 1) * DIRENT_SZ, dirent.as_bytes(), &self.block_device);
                // block_cache_sync_all();
            }
            root_disk_inode.size -= core::mem::size_of::<DirEntry>() as u32;
            // 这里没有处理size变小后，可能需要减少占用的block数的问题
            // 不过DiskInode.size的含义为文件内容的大小，所以最多有空间浪费，而不会导致问题，例如increase_size不会
            // 因为看到DiskInode.size较小而去错误扩容，increase_size的逻辑为通过索引去和new_size比对进行扩容，不会以为
            // DiskInode.size是capacity
        });
        
        Ok(())
    }

    /// 获取文件(指文件/目录)inode号，通过self的block_id和block_offset计算
    pub fn get_inode_id(&self) -> u64 {
        let fs = self.fs.lock();
        let inode_size = core::mem::size_of::<DiskInode>();
        let inodes_per_block = (BLOCK_SZ / inode_size) as u32; // 每个磁盘块多少个DiskInode
        let inode_id = (fs.inode_area_start_block as usize - self.block_id) * (inodes_per_block as usize) + self.block_offset / inode_size;
        inode_id as u64
    }

    /// 获取Inode的文件类型
    pub fn get_mode(&self) -> u32 {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_type = disk_inode.type_.clone();
            if file_type == DiskInodeType::Directory {
                return 0o040000; // StatMode中定义的魔数
            }
            else {
                return 0o100000;
            }
        })
    }

    /// 获取Inode的nlink
    pub fn get_nlink(&self) -> u32 {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            disk_inode.nlink
        })
    }

    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }

    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }

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
