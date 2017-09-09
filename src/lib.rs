use std::sync::{Mutex, Condvar};
use std::rc::Rc;
use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};

#[allow(dead_code)]
struct Operation {
    active : i32,
    waiting : Vec<Condvar>,
}
#[allow(dead_code)]
struct ReadWrite {
    reader : Operation,
    writer : Operation,
}
/// Provides a reader-writer lock to protect data of type `T`
pub struct RwLock<T> {
    lock : Mutex<()>,
    data: UnsafeCell<T>,
    global : UnsafeCell<G>,
    pref: Preference,
    order: Order,
}

#[derive(PartialEq)]
pub enum Preference {
    /// Readers-preferred
    /// * Readers must wait when a writer is active.
    /// * Writers must wait when a reader is active or waiting, or a writer is active.
    Reader,
    /// Writers-preferred:
    /// * Readers must wait when a writer is active or waiting.
    /// * Writer must wait when a reader or writer is active.
    Writer,
}

/// In which order to schedule threads
pub enum Order {
    /// First in first out
    Fifo,
    /// Last in first out
    Lifo,
}

struct G{
    reader_wait : Vec<Rc<Condvar>>,
    writer_wait :  Vec<Rc<Condvar>>,
    reader_active : i32,
    writer_active : i32,
}  //put all the global variable into a class/struct

impl<T> RwLock<T> {
    /// Constructs a new `RwLock`
    ///
    /// data: the shared object to be protected by this lock
    /// pref: which preference
    /// order: in which order to wake up the threads waiting on this lock
    pub fn new(data: T, pref: Preference, order: Order) -> RwLock<T> {
        RwLock{
            lock : Mutex::new(()),
            global : UnsafeCell::new({
                G{
                    reader_wait : Vec::new(),
                    writer_wait : Vec::new(),
                    reader_active : 0,
                    writer_active : 0,
                }
            }),
            data : UnsafeCell::new(data),
            pref : pref,
            order : order,


        }

    }

    /// Requests a read lock, waits when necessary, and wakes up as soon as the lock becomes available.
    ///
    /// Always returns Ok(_).
    /// (We declare this return type to be `Result` to be compatible with `std::sync::RwLock`)

    // read_wait return wait condition based on different preference
    // parallel read but sequencial write
    fn read_wait(&self) -> bool {
        let global = self.global.get();
        unsafe {
            let ref writer_wait = (*global).writer_wait;
            let writer_active = (*global).writer_active;

            match self.pref {
                Preference::Reader => {
                    if writer_active > 0 { return true; }
                    else{ return false; }
                },
                Preference::Writer => {
                    if writer_active > 0 || writer_wait.len() > 0 { return true; }
                    else{ return false; }
                },
            }
        }

    }

    pub fn read(&self) -> Result<RwLockReadGuard<T>, ()> {
        let mut guard = self.lock.lock().unwrap();
        let cond_var = Rc::new(Condvar::new());
        let global = self.global.get();
        unsafe {
            (*global).reader_wait.push(cond_var.clone());
        }
        while self.read_wait() {
            guard = cond_var.wait(guard).unwrap();
        }

        match self.order {
            Order::Fifo => {
                unsafe{
                    (*global).reader_wait.remove(0);
                }
            },
            Order::Lifo => {
                unsafe{
                    (*global).reader_wait.pop();
                }
            }
        }
        unsafe{
            (*global).reader_active += 1;
        }
        Ok(
            RwLockReadGuard {
                lock : &self
            }
        )
    }


    /// Requests a write lock, and waits when necessary.
    /// When the lock becomes available,
    /// * if `order == Order::Fifo`, wakes up the first thread
    /// * if `order == Order::Lifo`, wakes up the last thread
    ///
    /// Always returns Ok(_).
    #[allow(unused_variables)]
    fn write_wait(&self) -> bool {
        let global = self.global.get();
        unsafe {
            let ref writer_wait = (*global).writer_wait;
            let writer_active = (*global).writer_active;
            let ref reader_wait = (*global).reader_wait;
            let reader_active = (*global).reader_active;
            match self.pref {
                Preference::Reader => {
                    if writer_active > 0 || reader_wait.len() > 0 || reader_active > 0 { return true; }
                    else{ return false; }
                },
                Preference::Writer => {
                    if writer_active > 0 || reader_active > 0 { return true; }
                    else{ return false; }
                },
            }
        }
    }

    pub fn write(&self) -> Result<RwLockWriteGuard<T>, ()> {
        let mut guard = self.lock.lock().unwrap();
        let cond_var = Rc::new(Condvar::new());
        let global = self.global.get();

        unsafe{
            (*global).writer_wait.push(cond_var.clone());
            while self.write_wait() {
                guard = cond_var.wait(guard).unwrap();
            }
            match self.order {
                Order::Fifo => {
                    (*global).writer_wait.remove(0);
                },
                Order::Lifo => {
                    (*global).writer_wait.pop();
                }
            }
            (*global).writer_active += 1;
        }

        Ok(
            RwLockWriteGuard {
                lock : &self
            }
        )
    }

    pub fn notify_others(&self) {
        match self.pref {
            Preference::Reader => {
                unsafe {
                    let ref reader_wait = (*self.global.get()).reader_wait;
                    let ref writer_wait = (*self.global.get()).writer_wait;
                    match self.order {
                        Order::Fifo => {
                            if reader_wait.len() > 0 {
                                for i in 0..reader_wait.len() {
                                    reader_wait[i].notify_one();
                                }
                            }
                            else if writer_wait.len() > 0 {
                                writer_wait[0].notify_one(); //FIFO
                            }
                        },
                        Order::Lifo => {
                            if reader_wait.len() > 0 {
                                for i in (0..reader_wait.len()).rev() {
                                    reader_wait[i].notify_one();
                                }
                            }
                            else if writer_wait.len() > 0 {
                                writer_wait[writer_wait.len()-1].notify_one(); //FIFO
                            }
                        },
                    }
                }
            },
            Preference::Writer => {
                unsafe {
                    let ref reader_wait = (*self.global.get()).reader_wait;
                    let ref writer_wait = (*self.global.get()).writer_wait;
                    match self.order {
                        Order::Fifo => {
                            if writer_wait.len() > 0 {
                                writer_wait[0].notify_one();
                            }
                            else if reader_wait.len() > 0 {
                                for i in 0..reader_wait.len() {
                                    reader_wait[i].notify_one();
                                }
                            }
                        },
                        Order::Lifo => {
                            if writer_wait.len() > 0 {
                                writer_wait[writer_wait.len()-1].notify_one();
                            }
                            else if reader_wait.len() > 0 {
                                for i in (0..reader_wait.len()).rev() {
                                    reader_wait[i].notify_one();
                                }
                            }
                        },
                    }
                }
            },
        }
    }



}


/// Declares that it is safe to send and reference `RwLock` between threads safely
unsafe impl<T: Send + Sync> Send for RwLock<T> {}
unsafe impl<T: Send + Sync> Sync for RwLock<T> {}

/// A read guard for `RwLock`
pub struct RwLockReadGuard<'a, T: 'a> {
    lock: &'a RwLock<T>,
}

/// Provides access to the shared object
impl<'a, T> Deref for RwLockReadGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
            unsafe { &*self.lock.data.get() }
        }
}

/// Releases the read lock
#[allow(unused_variables)]
impl<'a, T> Drop for RwLockReadGuard<'a, T> {
    fn drop(&mut self) {
        let guard = self.lock.lock.lock().unwrap();
        unsafe {
            let global = self.lock.global.get();
            let ref reader_wait = (*global).reader_wait;
            let reader_active = (*global).reader_active;
            if reader_active > 0 {
                (*global).reader_active -= 1;
            }
            self.lock.notify_others();
         }
    }
}

/// A write guard for `RwLock`
pub struct RwLockWriteGuard<'a, T: 'a> {
    lock: &'a RwLock<T>,
}

/// Provides access to the shared object
impl<'a, T> Deref for RwLockWriteGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

/// Provides access to the shared object
impl<'a, T> DerefMut for RwLockWriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

/// Releases the write lock
#[allow(unused_variables)]
impl<'a, T> Drop for RwLockWriteGuard<'a, T> {
    fn drop(&mut self) {
        let guard = self.lock.lock.lock().unwrap();
        unsafe {
            let global = self.lock.global.get();
            let writer_active = (*global).writer_active;
            if writer_active > 0 {
                (*global).writer_active -= 1;
            }
            self.lock.notify_others();
        }
    }
}
