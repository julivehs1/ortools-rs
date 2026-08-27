use cpsat::{CpModelBuilder, Status};

#[test]
fn solves_a_linear_model() {
    let mut m = CpModelBuilder::default();
    let x = m.new_int_var(0..=10);
    let y = m.new_int_var(0..=10);
    m.add_le(x + y, 12);
    m.maximize(x * 2 + y);

    let r = m.solve();
    assert_eq!(r.status(), Status::Optimal);
    assert_eq!(x.value(&r), 10);
    assert_eq!(y.value(&r), 2);
}

#[test]
fn negated_literals_read_back_correctly() {
    let mut m = CpModelBuilder::default();
    let a = m.new_bool_var();
    m.add_eq(!a, 1);

    let r = m.solve();
    assert_eq!(r.status(), Status::Optimal);
    assert!(!a.value(&r));
    assert!((!a).value(&r));
}

#[test]
fn detects_infeasibility() {
    let mut m = CpModelBuilder::default();
    let x = m.new_int_var(0..=5);
    m.add_ge(x, 10);
    assert_eq!(m.solve().status(), Status::Infeasible);
}

#[test]
fn no_overlap_separates_two_tasks() {
    let mut m = CpModelBuilder::default();
    let (s1, e1) = (m.new_int_var(0..=10), m.new_int_var(0..=10));
    let (s2, e2) = (m.new_int_var(0..=10), m.new_int_var(0..=10));
    let i1 = m.new_interval_var(s1, 4, e1);
    let i2 = m.new_interval_var(s2, 4, e2);
    m.add_no_overlap([i1, i2]);

    let r = m.solve();
    assert_eq!(r.status(), Status::Optimal);
    let (s1, e1) = (s1.value(&r), e1.value(&r));
    let (s2, e2) = (s2.value(&r), e2.value(&r));
    assert_eq!(e1 - s1, 4);
    assert_eq!(e2 - s2, 4);
    assert!(
        e1 <= s2 || e2 <= s1,
        "intervals overlap: {s1}..{e1} and {s2}..{e2}"
    );
}

#[test]
fn cumulative_respects_capacity() {
    // Three unit-demand tasks of length 3 in a window of 4, capacity 2:
    // at most two may run at once, so the third cannot fit.
    let mut m = CpModelBuilder::default();
    let mut intervals = Vec::new();
    for _ in 0..3 {
        let s = m.new_int_var(0..=4);
        let e = m.new_int_var(0..=4);
        intervals.push(m.new_interval_var(s, 3, e));
    }
    let demands = vec![1i64.into(), 1i64.into(), 1i64.into()];
    m.add_cumulative(intervals, demands, 2);
    assert_eq!(m.solve().status(), Status::Infeasible);
}

#[test]
fn optional_intervals_may_be_absent() {
    let mut m = CpModelBuilder::default();
    let present = m.new_bool_var();
    let s = m.new_int_var(0..=2);
    let e = m.new_int_var(0..=2);
    // A size-5 task cannot fit in a horizon of 2, so it must be absent.
    let iv = m.new_optional_interval_var(s, 5, e, present);
    m.add_no_overlap([iv]);

    let r = m.solve();
    assert_eq!(r.status(), Status::Optimal);
    assert!(!present.value(&r));
}

#[test]
fn proto_mut_is_a_real_escape_hatch() {
    // Reach past the builder to post a constraint it does not wrap yet.
    use cpsat::proto;

    let mut m = CpModelBuilder::default();
    let x = m.new_int_var(0..=10);
    let y = m.new_int_var(0..=10);
    let z = m.new_int_var(0..=100);

    m.proto_mut().constraints.push(proto::ConstraintProto {
        constraint: Some(proto::constraint_proto::Constraint::IntProd(
            proto::LinearArgumentProto {
                target: Some(proto::LinearExpressionProto {
                    vars: vec![2],
                    coeffs: vec![1],
                    offset: 0,
                }),
                exprs: vec![
                    proto::LinearExpressionProto {
                        vars: vec![0],
                        coeffs: vec![1],
                        offset: 0,
                    },
                    proto::LinearExpressionProto {
                        vars: vec![1],
                        coeffs: vec![1],
                        offset: 0,
                    },
                ],
            },
        )),
        ..Default::default()
    });
    m.add_eq(x, 6);
    m.add_eq(y, 7);

    let r = m.solve();
    assert_eq!(r.status(), Status::Optimal);
    assert_eq!(z.value(&r), 42);
}

#[test]
fn validate_rejects_a_broken_model() {
    use cpsat::proto;
    let mut m = CpModelBuilder::default();
    // Domain bounds the wrong way round.
    m.proto_mut().variables.push(proto::IntegerVariableProto {
        name: String::new(),
        domain: vec![10, 0],
    });
    assert!(m.validate().is_err());
}
