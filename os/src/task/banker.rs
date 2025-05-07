//! 银行家算法，记录资源并检测安全状态

use core::cmp::max;

use alloc::vec;
use alloc::vec::Vec;

pub struct Banker {
    /// available向量
    pub available: Vec<i32>,

    /// allocation矩阵
    pub allocation: Vec<Vec<i32>>,

    /// need矩阵
    pub need: Vec<Vec<i32>>,
}

impl Banker {
    /// 创建
    pub fn new() -> Self {
        return Self {
            available: Vec::new(),
            allocation: Vec::new(),
            need: Vec::new()
        };
    }

    /// 创建新资源，在available向量里增加一行(如果需要的话)，allocation和need矩阵的行列懒增加，cnt为资源数量
    /// available不能懒扩容是因为其有初值cnt，而不像另外两个表初值为0
    pub fn add_new_resouce(&mut self, rid: usize, cnt: usize) {
        while self.available.len() <= rid {
            self.available.push(0);
        }
        self.available[rid] = cnt as i32;
    }

    /// （如有必要）扩容银行家的表格
    fn expand_table(&mut self, tid: usize, rid: usize) {
        let resource_count = max(rid, self.available.len());
        // 增加行
        while self.allocation.len() <= tid {
            self.allocation.push(vec![0; resource_count]);
        }
        while self.need.len() <= tid {
            self.need.push(vec![0; resource_count]);
        }
        // 每行都增加列，初值为0
        for i in 0..self.allocation.len() {
            while self.allocation[i].len() < resource_count {
                self.allocation[i].push(0);
            }
            while self.need[i].len() < resource_count {
                self.need[i].push(0);
            }
        }
    }

    /// 修改available，增量为delta（可以为负），add_available不用扩容，在资源创建（例如sys_mutex_create）时已经扩容了
    pub fn add_available(&mut self, rid: usize, delta: i32) {
        self.available[rid] += delta;
    }

    /// 修改allocation
    pub fn add_allocation(&mut self, tid: usize, rid: usize, delta: i32) {
        self.expand_table(tid, rid);
        self.allocation[tid][rid] += delta;
    }

    /// 修改need
    pub fn add_need(&mut self, tid: usize, rid: usize, delta: i32) {
        self.expand_table(tid, rid);
        self.need[tid][rid] += delta;
    }

    /// 用银行家算法检查是否安全状态
    pub fn banker_check(&self) -> bool {
        // debug!("=== start banker argorithm safety check ===");
        // debug!("available: {:?}", self.available);
        // debug!("allocation: {:?}", self.allocation);
        // debug!("need: {:?}", self.need);
        let resource_count = self.available.len();
        let thread_count = self.allocation.len();
        let mut cnt = 0; // 安全序列的线程数
        let mut work = self.available.clone();
        let mut finish = vec![false; thread_count]; // 能运行完的线程
        
        while cnt < thread_count {
            let mut have_finish = false; // 这轮是否有线程可以完成
            for i in 0..thread_count {
                if finish[i] {
                    continue;
                }
                if (0..resource_count).all(|j| work[j] >= self.need[i][j]) { // work可以满足这个线程的need
                    have_finish = true;
                    finish[i] = true;
                    cnt += 1;
                    for j in 0..resource_count {
                        work[j] += self.allocation[i][j]; // 线程释放了的资源加到work里
                    }
                    break; // 下一轮
                }
            }
            if !have_finish {
                break;
            }
        }

        let res;

        if cnt < thread_count { // 不能完成所有线程，系统不安全
            res = false;
        }
        else {
            res = true;
        }

        // debug!("banker argorithm safety check result: {}", res);
        res
    }
}