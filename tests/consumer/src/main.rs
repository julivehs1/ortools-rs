//! Builds and runs `cpsat` the way an actual user does.
//!
//! Deliberately its own package, outside the workspace. Cargo propagates
//! `rustc-link-lib` across package boundaries but not `rustc-link-arg`, so a
//! build script can look correct from inside the workspace — where tests and
//! examples belong to the same package and receive everything — while leaving a
//! downstream binary unable to link or start. This caught exactly that.

use cpsat::{CpModelBuilder, Status};

fn main() {
    let mut m = CpModelBuilder::default();
    let x = m.new_int_var(0..=10);
    let y = m.new_int_var(0..=10);
    m.add_le(x + y, 12);
    m.maximize(x * 2 + y);

    let r = m.solve();
    assert_eq!(r.status(), Status::Optimal, "solver did not reach optimality");
    assert_eq!((x.value(&r), y.value(&r)), (10, 2));

    // Scheduling too: it links a different part of OR-Tools.
    let mut m = CpModelBuilder::default();
    let s = m.new_int_var(0..=10);
    let e = m.new_int_var(0..=10);
    let iv = m.new_interval_var(s, 4, e);
    m.add_no_overlap([iv]);
    assert_eq!(m.solve().status(), Status::Optimal);

    println!("consumer ok");
}
