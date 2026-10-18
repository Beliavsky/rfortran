mod bindings;

fn main() {
    let count = 10_000;
    let mut sum = 0.0;
    let mut sum_squares = 0.0;
    // rnorm.f90 has SAVE state. All calls here occur on this one thread.
    for _ in 0..count {
        let value = unsafe { bindings::random_normal_scalar() };
        assert!(value.is_finite());
        sum += value;
        sum_squares += value * value;
    }
    let mean = sum / f64::from(count);
    let variance = sum_squares / f64::from(count) - mean * mean;
    println!("Rust called existing rnorm.f90: {count} finite normal samples.");
    println!("Sample mean = {mean:.5}, sample standard deviation = {:.5}", variance.sqrt());
}
