//! Classic job-shop scheduling.
//!
//! Each job is a fixed sequence of tasks; each task occupies one machine for a
//! fixed duration; a machine runs one task at a time. Minimise the makespan.
//!
//! This is the canonical use for interval variables, and the reason they are
//! worth having: the machine constraint is one `add_no_overlap` call rather
//! than a quadratic pile of disjunctions.

use cpsat::{CpModelBuilder, LinearExpr, Status};

/// `jobs[j][t] = (machine, duration)`
const JOBS: &[&[(usize, i64)]] = &[
    &[(0, 3), (1, 2), (2, 2)],
    &[(0, 2), (2, 1), (1, 4)],
    &[(1, 4), (2, 3)],
];

fn main() {
    let horizon: i64 = JOBS.iter().flat_map(|j| j.iter().map(|t| t.1)).sum();
    let machines = 1 + JOBS
        .iter()
        .flat_map(|j| j.iter().map(|t| t.0))
        .max()
        .unwrap();

    let mut m = CpModelBuilder::default();
    let mut per_machine = vec![Vec::new(); machines];
    let mut starts = Vec::new();
    let mut ends = Vec::new();

    for job in JOBS {
        let mut previous_end = None;
        let mut job_starts = Vec::new();
        let mut job_ends = Vec::new();

        for &(machine, duration) in job.iter() {
            let start = m.new_int_var(0..=horizon);
            let end = m.new_int_var(0..=horizon);
            let interval = m.new_interval_var(start, duration, end);
            per_machine[machine].push(interval);

            // Tasks within a job run in order.
            if let Some(prev) = previous_end {
                m.add_ge(start, prev);
            }
            previous_end = Some(end);
            job_starts.push(start);
            job_ends.push(end);
        }
        starts.push(job_starts);
        ends.push(job_ends);
    }

    for intervals in per_machine {
        m.add_no_overlap(intervals);
    }

    // Makespan: at least as large as every job's final end.
    let makespan = m.new_int_var(0..=horizon);
    for job_ends in &ends {
        m.add_ge(makespan, *job_ends.last().unwrap());
    }
    m.minimize(makespan);

    m.validate().expect("model should be valid");

    let response = m.solve();
    println!("status: {:?}", response.status());
    assert_eq!(response.status(), Status::Optimal);

    println!("makespan: {}", makespan.value(&response));
    for (j, job) in JOBS.iter().enumerate() {
        let line: Vec<String> = job
            .iter()
            .enumerate()
            .map(|(t, (machine, duration))| {
                let s = starts[j][t].value(&response);
                format!("M{machine}@{s}..{}", s + duration)
            })
            .collect();
        println!("  job {j}: {}", line.join("  "));
    }

    // Silence the unused-import lint in the common case; LinearExpr is part of
    // the public surface this example is meant to exercise.
    let _ = LinearExpr::zero();
}
