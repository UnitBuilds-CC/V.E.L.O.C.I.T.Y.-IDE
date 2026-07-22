#[derive(Debug, Clone)]
pub struct FuzzReport {
    pub total_runs: u32,
    pub passes: u32,
    pub failures: u32,
    pub counter_example: Option<String>,
}

pub struct PropertyFuzzer;

impl PropertyFuzzer {
    pub fn fuzz_property<F>(runs: u32, mut prop: F) -> FuzzReport
    where
        F: FnMut(u64) -> bool,
    {
        let mut passes = 0;
        let mut failures = 0;
        let mut counter_example = None;

        for i in 0..runs {
            let seed = (i as u64).wrapping_mul(0x9E3779B97F4A7C15);
            if prop(seed) {
                passes += 1;
            } else {
                failures += 1;
                if counter_example.is_none() {
                    counter_example = Some(format!("Seed failure at input {}", seed));
                }
            }
        }

        FuzzReport {
            total_runs: runs,
            passes,
            failures,
            counter_example,
        }
    }
}
