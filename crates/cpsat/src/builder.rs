//! Building a `CpModelProto` without writing protobuf by hand.
//!
//! # Why intervals are their own type
//!
//! CP-SAT has two index spaces. Integer and boolean variables index into
//! `CpModelProto::variables`; an *interval* is not a variable at all but a
//! constraint, and `no_overlap` / `cumulative` reference it by its index into
//! `CpModelProto::constraints`. Conflating the two silently produces models
//! that validate and then solve the wrong problem, so [`IntervalVar`] is a
//! distinct handle that cannot be used where an [`IntVar`] is expected.

use crate::proto;

/// A model under construction.
#[derive(Default, Clone, Debug)]
pub struct CpModelBuilder {
    proto: proto::CpModelProto,
}

/// A boolean variable. Negate it with `!`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct BoolVar(i32);

/// An integer variable over a finite domain.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct IntVar(i32);

/// A start/size/end triple that scheduling constraints operate on.
///
/// Obtained from [`CpModelBuilder::new_interval_var`]. Not an [`IntVar`]: see
/// the module docs for why.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct IntervalVar(i32);

/// Handle to a posted constraint, for attaching a name or an enforcement literal.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Constraint(usize);

/// A weighted sum of variables plus a constant.
#[derive(Default, Clone, PartialEq, Eq, Debug)]
pub struct LinearExpr {
    vars: Vec<i32>,
    coeffs: Vec<i64>,
    offset: i64,
}

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

impl CpModelBuilder {
    /// The model as a protobuf.
    pub fn proto(&self) -> &proto::CpModelProto {
        &self.proto
    }

    /// The model as a mutable protobuf.
    ///
    /// This is a deliberate escape hatch. Anything CP-SAT can express but this
    /// builder has not wrapped yet is reachable here, so a missing convenience
    /// method is never a dead end.
    pub fn proto_mut(&mut self) -> &mut proto::CpModelProto {
        &mut self.proto
    }

    /// Add a boolean variable.
    pub fn new_bool_var(&mut self) -> BoolVar {
        BoolVar(self.push_var(vec![0, 1], String::new()))
    }

    /// Add an integer variable over the inclusive range `domain`.
    pub fn new_int_var(&mut self, domain: std::ops::RangeInclusive<i64>) -> IntVar {
        IntVar(self.push_var(vec![*domain.start(), *domain.end()], String::new()))
    }

    /// Add an integer variable over a union of inclusive ranges.
    pub fn new_int_var_from_ranges(
        &mut self,
        ranges: impl IntoIterator<Item = std::ops::RangeInclusive<i64>>,
    ) -> IntVar {
        let domain = ranges
            .into_iter()
            .flat_map(|r| [*r.start(), *r.end()])
            .collect();
        IntVar(self.push_var(domain, String::new()))
    }

    fn push_var(&mut self, domain: Vec<i64>, name: String) -> i32 {
        let index = self.proto.variables.len() as i32;
        self.proto
            .variables
            .push(proto::IntegerVariableProto { name, domain });
        index
    }

    fn push_constraint(&mut self, c: proto::constraint_proto::Constraint) -> Constraint {
        let index = self.proto.constraints.len();
        self.proto.constraints.push(proto::ConstraintProto {
            constraint: Some(c),
            ..Default::default()
        });
        Constraint(index)
    }

    /// Give a posted constraint a name, for readable stats and error messages.
    pub fn set_constraint_name(&mut self, c: Constraint, name: impl Into<String>) {
        self.proto.constraints[c.0].name = name.into();
    }

    /// Make a constraint conditional: it holds only when every literal does.
    pub fn only_enforce_if(
        &mut self,
        c: Constraint,
        literals: impl IntoIterator<Item = BoolVar>,
    ) -> Constraint {
        self.proto.constraints[c.0]
            .enforcement_literal
            .extend(literals.into_iter().map(|b| b.0));
        c
    }

    // -- scheduling ---------------------------------------------------------

    /// Add an interval with `start + size == end`.
    ///
    /// All three may be arbitrary linear expressions, so a task whose duration
    /// depends on another variable needs no auxiliary encoding.
    pub fn new_interval_var(
        &mut self,
        start: impl Into<LinearExpr>,
        size: impl Into<LinearExpr>,
        end: impl Into<LinearExpr>,
    ) -> IntervalVar {
        let c = self.push_constraint(proto::constraint_proto::Constraint::Interval(
            proto::IntervalConstraintProto {
                start: Some(start.into().into()),
                size: Some(size.into().into()),
                end: Some(end.into().into()),
            },
        ));
        IntervalVar(c.0 as i32)
    }

    /// Add an interval that only exists when `presence` is true.
    ///
    /// Optional intervals are how you model "this task might not be scheduled
    /// at all" — absent ones are ignored by `no_overlap` and `cumulative`.
    pub fn new_optional_interval_var(
        &mut self,
        start: impl Into<LinearExpr>,
        size: impl Into<LinearExpr>,
        end: impl Into<LinearExpr>,
        presence: BoolVar,
    ) -> IntervalVar {
        let iv = self.new_interval_var(start, size, end);
        self.proto.constraints[iv.0 as usize]
            .enforcement_literal
            .push(presence.0);
        iv
    }

    /// Forbid any two of `intervals` from overlapping in time.
    pub fn add_no_overlap(
        &mut self,
        intervals: impl IntoIterator<Item = IntervalVar>,
    ) -> Constraint {
        self.push_constraint(proto::constraint_proto::Constraint::NoOverlap(
            proto::NoOverlapConstraintProto {
                intervals: intervals.into_iter().map(|i| i.0).collect(),
            },
        ))
    }

    /// Cap the total demand of overlapping intervals at `capacity`.
    ///
    /// # Panics
    ///
    /// If `intervals` and `demands` differ in length — CP-SAT requires them to
    /// correspond pairwise, and a silent mismatch would solve a different model.
    pub fn add_cumulative(
        &mut self,
        intervals: impl IntoIterator<Item = IntervalVar>,
        demands: impl IntoIterator<Item = LinearExpr>,
        capacity: impl Into<LinearExpr>,
    ) -> Constraint {
        let intervals: Vec<i32> = intervals.into_iter().map(|i| i.0).collect();
        let demands: Vec<proto::LinearExpressionProto> =
            demands.into_iter().map(Into::into).collect();
        assert_eq!(
            intervals.len(),
            demands.len(),
            "add_cumulative: {} intervals but {} demands",
            intervals.len(),
            demands.len()
        );
        self.push_constraint(proto::constraint_proto::Constraint::Cumulative(
            proto::CumulativeConstraintProto {
                capacity: Some(capacity.into().into()),
                intervals,
                demands,
            },
        ))
    }

    // -- linear -------------------------------------------------------------

    /// Constrain a linear expression to lie within an inclusive range.
    pub fn add_linear_constraint(
        &mut self,
        expr: impl Into<LinearExpr>,
        domain: std::ops::RangeInclusive<i64>,
    ) -> Constraint {
        let expr = expr.into();
        // The proto has no offset on linear constraints, so fold it into the
        // bounds instead.
        let lo = domain.start().saturating_sub(expr.offset);
        let hi = domain.end().saturating_sub(expr.offset);
        self.push_constraint(proto::constraint_proto::Constraint::Linear(
            proto::LinearConstraintProto {
                vars: expr.vars,
                coeffs: expr.coeffs,
                domain: vec![lo, hi],
            },
        ))
    }

    /// `lhs <= rhs`
    pub fn add_le(&mut self, lhs: impl Into<LinearExpr>, rhs: impl Into<LinearExpr>) -> Constraint {
        self.add_linear_constraint(lhs.into() - rhs.into(), i64::MIN..=0)
    }

    /// `lhs >= rhs`
    pub fn add_ge(&mut self, lhs: impl Into<LinearExpr>, rhs: impl Into<LinearExpr>) -> Constraint {
        self.add_linear_constraint(lhs.into() - rhs.into(), 0..=i64::MAX)
    }

    /// `lhs == rhs`
    pub fn add_eq(&mut self, lhs: impl Into<LinearExpr>, rhs: impl Into<LinearExpr>) -> Constraint {
        self.add_linear_constraint(lhs.into() - rhs.into(), 0..=0)
    }

    /// Every variable takes a distinct value.
    pub fn add_all_different(&mut self, exprs: impl IntoIterator<Item = LinearExpr>) -> Constraint {
        self.push_constraint(proto::constraint_proto::Constraint::AllDiff(
            proto::AllDifferentConstraintProto {
                exprs: exprs.into_iter().map(Into::into).collect(),
            },
        ))
    }

    // -- objective ----------------------------------------------------------

    /// Minimise a linear expression.
    pub fn minimize(&mut self, expr: impl Into<LinearExpr>) {
        let expr = expr.into();
        self.proto.objective = Some(proto::CpObjectiveProto {
            vars: expr.vars,
            coeffs: expr.coeffs,
            offset: expr.offset as f64,
            ..Default::default()
        });
    }

    /// Maximise a linear expression.
    ///
    /// CP-SAT only minimises, so this negates the objective and flips the sign
    /// back via `scaling_factor` when reporting bounds.
    pub fn maximize(&mut self, expr: impl Into<LinearExpr>) {
        let expr = expr.into();
        self.proto.objective = Some(proto::CpObjectiveProto {
            vars: expr.vars,
            coeffs: expr.coeffs.iter().map(|c| -c).collect(),
            offset: -(expr.offset as f64),
            scaling_factor: -1.0,
            ..Default::default()
        });
    }

    // -- solving ------------------------------------------------------------

    /// Solve with default parameters.
    pub fn solve(&self) -> proto::CpSolverResponse {
        crate::solve(&self.proto)
    }

    /// Solve with explicit parameters.
    pub fn solve_with_parameters(&self, params: &proto::SatParameters) -> proto::CpSolverResponse {
        crate::solve_with_parameters(&self.proto, params)
    }

    /// Human-readable summary of the model's size and structure.
    pub fn stats(&self) -> String {
        crate::model_stats(&self.proto)
    }

    /// `Ok(())` if the model is well formed, otherwise CP-SAT's explanation.
    pub fn validate(&self) -> Result<(), String> {
        crate::validate(&self.proto)
    }
}

// ---------------------------------------------------------------------------
// Reading solutions
// ---------------------------------------------------------------------------

impl IntVar {
    /// This variable's value in a solved response.
    pub fn value(self, response: &proto::CpSolverResponse) -> i64 {
        response.solution[self.0 as usize]
    }
}

impl BoolVar {
    /// This literal's value in a solved response.
    ///
    /// Handles negated literals, which are encoded as `-index - 1`.
    pub fn value(self, response: &proto::CpSolverResponse) -> bool {
        if self.0 >= 0 {
            response.solution[self.0 as usize] != 0
        } else {
            response.solution[(-self.0 - 1) as usize] == 0
        }
    }
}

impl std::ops::Not for BoolVar {
    type Output = BoolVar;
    fn not(self) -> BoolVar {
        BoolVar(-self.0 - 1)
    }
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

impl LinearExpr {
    /// The constant zero.
    pub fn zero() -> Self {
        Self::default()
    }

    /// Sum of `terms`.
    pub fn sum(terms: impl IntoIterator<Item = impl Into<LinearExpr>>) -> Self {
        terms
            .into_iter()
            .fold(Self::zero(), |acc, t| acc + t.into())
    }

    /// Weighted sum of `(term, weight)` pairs.
    pub fn weighted_sum(terms: impl IntoIterator<Item = (impl Into<LinearExpr>, i64)>) -> Self {
        terms
            .into_iter()
            .fold(Self::zero(), |acc, (t, w)| acc + t.into() * w)
    }
}

impl From<i64> for LinearExpr {
    fn from(offset: i64) -> Self {
        Self {
            offset,
            ..Default::default()
        }
    }
}

impl From<IntVar> for LinearExpr {
    fn from(v: IntVar) -> Self {
        Self {
            vars: vec![v.0],
            coeffs: vec![1],
            offset: 0,
        }
    }
}

impl From<BoolVar> for LinearExpr {
    fn from(b: BoolVar) -> Self {
        // A negated literal !x is the expression 1 - x.
        if b.0 >= 0 {
            Self {
                vars: vec![b.0],
                coeffs: vec![1],
                offset: 0,
            }
        } else {
            Self {
                vars: vec![-b.0 - 1],
                coeffs: vec![-1],
                offset: 1,
            }
        }
    }
}

impl From<LinearExpr> for proto::LinearExpressionProto {
    fn from(e: LinearExpr) -> Self {
        Self {
            vars: e.vars,
            coeffs: e.coeffs,
            offset: e.offset,
        }
    }
}

impl<T: Into<LinearExpr>> std::ops::Add<T> for LinearExpr {
    type Output = LinearExpr;
    fn add(mut self, rhs: T) -> LinearExpr {
        let rhs = rhs.into();
        self.vars.extend(rhs.vars);
        self.coeffs.extend(rhs.coeffs);
        self.offset += rhs.offset;
        self
    }
}

impl<T: Into<LinearExpr>> std::ops::Sub<T> for LinearExpr {
    type Output = LinearExpr;
    fn sub(self, rhs: T) -> LinearExpr {
        self + (-rhs.into())
    }
}

impl std::ops::Neg for LinearExpr {
    type Output = LinearExpr;
    fn neg(mut self) -> LinearExpr {
        for c in &mut self.coeffs {
            *c = -*c;
        }
        self.offset = -self.offset;
        self
    }
}

impl std::ops::Mul<i64> for LinearExpr {
    type Output = LinearExpr;
    fn mul(mut self, rhs: i64) -> LinearExpr {
        for c in &mut self.coeffs {
            *c *= rhs;
        }
        self.offset *= rhs;
        self
    }
}

/// Generate the same operator set for the variable handles, so `x + y * 2`
/// works without an explicit conversion at every call site.
macro_rules! forward_ops {
    ($($t:ty),*) => {$(
        impl<T: Into<LinearExpr>> std::ops::Add<T> for $t {
            type Output = LinearExpr;
            fn add(self, rhs: T) -> LinearExpr { LinearExpr::from(self) + rhs }
        }
        impl<T: Into<LinearExpr>> std::ops::Sub<T> for $t {
            type Output = LinearExpr;
            fn sub(self, rhs: T) -> LinearExpr { LinearExpr::from(self) - rhs }
        }
        impl std::ops::Mul<i64> for $t {
            type Output = LinearExpr;
            fn mul(self, rhs: i64) -> LinearExpr { LinearExpr::from(self) * rhs }
        }
        impl std::ops::Neg for $t {
            type Output = LinearExpr;
            fn neg(self) -> LinearExpr { -LinearExpr::from(self) }
        }
    )*};
}

forward_ops!(IntVar, BoolVar);
