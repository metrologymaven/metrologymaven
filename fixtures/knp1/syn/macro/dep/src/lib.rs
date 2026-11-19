//! AC-2: a dependency defines a macro whose body fuses; the finding is at the body line.

#[macro_export]
macro_rules! m {
    ($x:expr) => {
        $x.mul_add(1.0, 2.0)
    };
}
