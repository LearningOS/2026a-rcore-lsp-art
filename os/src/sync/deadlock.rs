//! Codex 协助实现；按官方第八章的 Available/Allocation/Need 算法检测。
use crate::task::{current_process, current_task, TaskControlBlock};
use alloc::{collections::BTreeMap, vec, vec::Vec};

/// 一类同步资源的分配与等待记录。
#[derive(Default)]
pub struct ResourceTable {
    available: Vec<usize>,
    allocation: BTreeMap<usize, Vec<usize>>,
    need: BTreeMap<usize, Vec<usize>>,
}

impl ResourceTable {
    /// 新增或复用资源编号。
    pub fn create(&mut self, id: usize, count: usize) {
        if self.available.len() <= id {
            self.available.resize(id + 1, 0);
        }
        self.available[id] = count;
        for row in self.allocation.values_mut() {
            row.resize(self.available.len(), 0);
        }
        for row in self.need.values_mut() {
            row.resize(self.available.len(), 0);
        }
    }
    fn ensure_thread(&mut self, tid: usize) {
        let width = self.available.len();
        self.allocation.entry(tid).or_insert_with(|| vec![0; width]);
        self.need.entry(tid).or_insert_with(|| vec![0; width]);
    }
    fn safe(&self) -> bool {
        let mut work = self.available.clone();
        let mut finish = vec![false; self.allocation.len()];
        loop {
            let mut changed = false;
            for (i, (tid, allocated)) in self.allocation.iter().enumerate() {
                if !finish[i]
                    && self.need[tid]
                        .iter()
                        .zip(&work)
                        .all(|(need, free)| need <= free)
                {
                    for (free, held) in work.iter_mut().zip(allocated) {
                        *free += held;
                    }
                    finish[i] = true;
                    changed = true;
                }
            }
            if !changed {
                return finish.iter().all(|done| *done);
            }
        }
    }
    fn request(&mut self, tid: usize, id: usize, enabled: bool) -> bool {
        self.ensure_thread(tid);
        self.need.get_mut(&tid).unwrap()[id] += 1;
        if enabled && !self.safe() {
            self.need.get_mut(&tid).unwrap()[id] -= 1;
            false
        } else {
            true
        }
    }
    fn grant(&mut self, tid: usize, id: usize) {
        self.ensure_thread(tid);
        self.available[id] -= 1;
        self.need.get_mut(&tid).unwrap()[id] -= 1;
        self.allocation.get_mut(&tid).unwrap()[id] += 1;
    }
    fn release(&mut self, tid: usize, id: usize) {
        self.ensure_thread(tid);
        let held = &mut self.allocation.get_mut(&tid).unwrap()[id];
        // 信号量允许生产者 up，即使它没有执行过 down。
        *held = held.saturating_sub(1);
        self.available[id] += 1;
    }
}

/// 读取线程编号，借用不跨越阻塞或调度。
pub fn task_id(task: &TaskControlBlock) -> usize {
    task.inner_exclusive_access().res.as_ref().unwrap().tid
}
fn with_table<T>(semaphore: bool, f: impl FnOnce(&mut ResourceTable, bool) -> T) -> T {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let enabled = inner.deadlock_enabled;
    let table = if semaphore {
        &mut inner.sem_resources
    } else {
        &mut inner.mutex_resources
    };
    f(table, enabled)
}
/// 登记申请并在启用检测时判断是否会形成死锁。
pub fn request(semaphore: bool, id: usize) -> bool {
    let tid = task_id(&current_task().unwrap());
    with_table(semaphore, |table, enabled| table.request(tid, id, enabled))
}
/// 在实际取得资源或唤醒时，将等待量转为分配量。
pub fn grant(semaphore: bool, id: usize, tid: usize) {
    with_table(semaphore, |table, _| table.grant(tid, id));
}
/// 更新释放记录；唤醒排队线程时再立即转交资源。
pub fn release(semaphore: bool, id: usize) {
    let tid = task_id(&current_task().unwrap());
    with_table(semaphore, |table, _| table.release(tid, id));
}
