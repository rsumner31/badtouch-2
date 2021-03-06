use ctx::Script;
use threadpool::ThreadPool;
use keyboard;
use errors::Result;
use std::sync::{mpsc, Arc, Mutex, Condvar};

#[derive(Debug)]
pub struct Attempt {
    pub user: Arc<String>,
    pub password: Arc<String>,
    pub script: Arc<Script>,
    pub ttl: u8,
}

impl Attempt {
    #[inline]
    pub fn new(user: &Arc<String>, password: &Arc<String>, script: &Arc<Script>) -> Attempt {
        Attempt {
            user: user.clone(),
            password: password.clone(),
            script: script.clone(),
            ttl: 5,
        }
    }

    #[inline]
    pub fn run(self, tx: mpsc::Sender<Msg>) {
        let result = self.script.run_once(&self.user, &self.password);
        tx.send(Msg::Attempt(self, result)).expect("failed to send result");
    }
}

#[derive(Debug)]
pub enum Msg {
    Attempt(Attempt, Result<bool>),
    Key(keyboard::Key),
}

pub struct Scheduler {
    pool: ThreadPool,
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
    num_threads: usize,
    inflight: usize,
    pause_trigger: Arc<(Mutex<bool>, Condvar)>,
}

impl Scheduler {
    #[inline]
    pub fn new(workers: usize) -> Scheduler {
        let (tx, rx) = mpsc::channel();
        Scheduler {
            pool: ThreadPool::new(workers),
            tx,
            rx,
            num_threads: workers,
            inflight: 0,
            pause_trigger: Arc::new((Mutex::new(true), Condvar::new())),
        }
    }

    #[inline]
    pub fn pause(&mut self) {
        let &(ref lock, _) = &*self.pause_trigger;
        let mut paused = lock.lock().unwrap();
        *paused = true;
    }

    #[inline]
    pub fn resume(&mut self) {
        let &(ref lock, ref cvar) = &*self.pause_trigger;
        let mut paused = lock.lock().unwrap();
        *paused = false;
        cvar.notify_all();
    }

    #[inline]
    pub fn incr(&mut self) -> usize {
        self.num_threads += 1;
        self.pool.set_num_threads(self.num_threads);
        self.num_threads
    }

    #[inline]
    pub fn decr(&mut self) -> usize {
        if self.num_threads == 1 {
            return self.num_threads;
        }

        self.num_threads -= 1;
        self.pool.set_num_threads(self.num_threads);
        self.num_threads
    }

    #[inline]
    pub fn tx(&self) -> mpsc::Sender<Msg> {
        self.tx.clone()
    }

    #[inline]
    pub fn max_count(&self) -> usize {
        self.pool.max_count()
    }

    #[inline]
    pub fn has_work(&self) -> bool {
        self.inflight > 0
    }

    #[inline]
    pub fn run(&mut self, attempt: Attempt) {
        let tx = self.tx.clone();
        let pause_trigger = self.pause_trigger.clone();
        self.inflight += 1;

        self.pool.execute(move || {
            // verify the pause trigger isn't enabled
            // if it is locked, block until it is unlocked
            let &(ref lock, ref cvar) = &*pause_trigger;
            {
                let mut paused = lock.lock().unwrap();
                while *paused {
                    paused = cvar.wait(paused).unwrap();
                }
            }
            attempt.run(tx);
        });
    }

    #[inline]
    pub fn recv(&mut self) -> Msg {
        self.inflight -= 1;
        self.rx.recv().unwrap()
    }
}
