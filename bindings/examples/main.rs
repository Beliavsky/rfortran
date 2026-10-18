mod bindings;

fn main() {
    // This demo's reviewed Fortran routines have no retained pointers or shared state.
    unsafe {
        assert_eq!(bindings::twice(3.5), 7.0);
        assert_eq!(bindings::twice_single(1.25), 2.5);
        let mut x = vec![1.0, 2.0, 3.0];
        bindings::affine(&mut x, 2.0, 1.0);
        assert_eq!(x, [3.0, 5.0, 7.0]);
        let mut z = vec![0.0; 3];
        bindings::add(&x, &[10.0, 20.0, 30.0], &mut z);
        assert_eq!(z, [13.0, 25.0, 37.0]);
        assert_eq!(bindings::total(&z), 75.0);
        assert_eq!(bindings::total(&[]), 0.0);
        let mut empty = [];
        bindings::affine(&mut empty, 2.0, 1.0);
        bindings::add(&[], &[], &mut empty);
        let mut code = 99;
        bindings::classify(-2.0, &mut code);
        assert_eq!(code, -1);
        let mut integer = 4_000_000_000_i64;
        bindings::increment(&mut integer);
        assert_eq!(integer, 4_000_000_001);
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let mismatch = std::panic::catch_unwind(|| {
            bindings::add(&[1.0], &[2.0, 3.0], &mut [0.0]);
        });
        std::panic::set_hook(hook);
        assert!(mismatch.is_err());
        println!("Rust called Fortran: scalar, array, output, integer, empty-array, and length checks passed.");
    }
}
