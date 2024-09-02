extern "C" {
    static stests: usize;
    static etests: usize;
}

pub struct TestDescription {
    pub name: &'static str,
    pub module: &'static str,
    pub func: fn() -> bool,
}

#[macro_export]
macro_rules! test_assert {
    ($cond:expr) => {
        if !$cond {
            logln!("\nCondition failed: `{}` at {}:{}", stringify!($cond), file!(), line!());
            return false;
        }
    };
}

#[macro_export]
macro_rules! test_assert_ne {
    ($e1:expr, $e2:expr) => {
        if $e1 == $e2 {
            logln!("\nTest assert failure at {}:{:?} ", file!(), line!());
            logln!("Condition failed: `{:?} != {:?}`", $e1, $e2);
            return false;
        }
    };
}

#[macro_export]
macro_rules! test_assert_eq {
    ($e1:expr, $e2:expr) => {
        if $e1 != $e2 {
            logln!("\nTest assert failure at {}:{:?} ", file!(), line!());
            logln!("Condition failed: `{:?} == {:?}`\n", $e1, $e2);
            return false;
        }
    };
}

pub fn tests_main() {
    tests_runner(&[()]);
}

pub fn tests_runner(_tests: &[()]) {
    use core::fmt::Write;

    let kernel_tests = unsafe {
        core::slice::from_raw_parts(
            &stests as *const usize as *const u8 as *const TestDescription,
            (&etests as *const usize as usize - &stests as *const usize as usize)
                / core::mem::size_of::<TestDescription>(),
        )
    };

    for i in kernel_tests {
        crate::logln!("Running test {}::{}", i.module, i.name);

        let success = (i.func)();

        if !success {
            crate::logln!("[FAIL]\n");
        } else {
            crate::logln!("[SUCCESS]\n");
        }
    }
}
