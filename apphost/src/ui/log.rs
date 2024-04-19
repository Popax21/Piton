use std::{marker::PhantomData, sync::RwLock};

#[macro_export]
macro_rules! log {
    ($($msg_arg:tt)+) => {
        if !$crate::cfg::IS_QUIET {
            let msg = format!($($msg_arg)+);
            let msg = format!("[PITON] {msg}");

            let log_guard = $crate::ui::log::LOG_FNC.read().unwrap();
            if let Some(log_fnc) = *log_guard {
                log_fnc(&msg);
            } else {
                println!("{msg}");
            }
        }
    };
}

pub type LogFunc = dyn Fn(&str) + Sync;
pub static LOG_FNC: RwLock<Option<&LogFunc>> = RwLock::new(None);

pub struct LogHook<'a, F: Fn(&str) + Sync + 'a>(PhantomData<&'a F>);

impl<'a, F: Fn(&str) + Sync + 'a> LogHook<'a, F> {
    pub fn create(fnc: &'a F) -> LogHook<'a, F> {
        //Apply the log hook
        //The transmute is safe because the returned LogHook struct will be dropped before the function reference, and it will remove the hook again
        let mut log_fnc = LOG_FNC.write().unwrap();
        if log_fnc.is_some() {
            panic!("attempted creation of two simultaneous log hooks");
        }
    
        *log_fnc = Some(unsafe { std::mem::transmute::<&'a (dyn Fn(&str) + Sync), &'static (dyn Fn(&str) + Sync)>(fnc) });
        
        LogHook(PhantomData)
    }
}

impl<F: Fn(&str) + Sync> Drop for LogHook<'_, F> {
    fn drop(&mut self) {
        let mut log_fnc = LOG_FNC.write().unwrap();
        assert!(log_fnc.is_some());
        *log_fnc = None;
    }
}