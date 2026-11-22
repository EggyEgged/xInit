//! State-machine model of the pidfd-tracked Service API (rule 6): B-M0-029, B-M0-030,
//! B-M0-031, I-1. No property-testing crate is a dependency of xinit-tests, so the
//! operation sequences come from a fixed-seed xorshift generator; every case is
//! reproducible from its seed, which failure messages print.
//!
//! Reference model, written from the spec only:
//! - Running: the process is alive. `signal(s)` -> Ok, the process becomes Dying({s}).
//!   `stop(5 s)` on `/usr/bin/sleep 60` -> Ok{Signaled(15), escalated:false}, Reaped.
//! - Dying(sent): a fatal signal was sent, not yet reaped. `signal(s)` -> Ok or
//!   Err(Os{ESRCH=3}) (the spec gives Exited only after the reap). `wait()` ->
//!   Signaled(n) with n one of the signals sent; then Reaped(that termination).
//! - Reaped(t): `signal(_)` -> Err(Exited); `wait()` -> Ok(t) again, without blocking.
//!
//! Operations the spec leaves open are never generated: `wait` while Running (blocks
//! forever), `stop` while Dying or Reaped.
//! Invariants after every step: `pid()` never changes; Running => /proc shows the same
//! process alive; Reaped => /proc/<pid> no longer refers to it.

use super::common::{child_lock_shared, is_live, require_prereqs, same_process_exists, start_time};
use std::time::Duration;
use xinit_api::{ExecutionSection, Service, Signal, SignalError, StopOutcome, Termination};

#[derive(Debug, Clone, PartialEq)]
enum Model {
    Running,
    Dying(Vec<i32>),
    Reaped(Termination),
}

#[derive(Debug, Clone, Copy)]
enum Op {
    Signal(Signal),
    Wait,
    Stop,
    Pid,
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[(self.next() % xs.len() as u64) as usize]
    }
}

fn signum(s: Signal) -> i32 {
    match s {
        Signal::Term => 15,
        Signal::Kill => 9,
        Signal::Int => 2,
    }
}

const SIGNALS: [Signal; 3] = [Signal::Term, Signal::Kill, Signal::Int];

fn choose(rng: &mut Rng, m: &Model) -> Op {
    let sig = Op::Signal(rng.pick(&SIGNALS));
    match m {
        Model::Running => rng.pick(&[sig, Op::Stop, Op::Pid]),
        Model::Dying(_) | Model::Reaped(_) => rng.pick(&[sig, Op::Wait, Op::Pid]),
    }
}

fn run_case(seed: u64, steps: usize) {
    let mut rng = Rng(seed);
    let exec = ExecutionSection {
        command: vec!["/usr/bin/sleep".into(), "60".into()],
        user: None,
    };
    let mut svc =
        Service::start(&exec).unwrap_or_else(|e| panic!("seed {seed}: start failed: {e:?}"));
    let pid = svc.pid();
    let st = start_time(pid)
        .unwrap_or_else(|| panic!("seed {seed}: /proc/{pid} missing right after start"));
    let mut model = Model::Running;
    let mut trace: Vec<String> = Vec::new();
    for _ in 0..steps {
        let op = choose(&mut rng, &model);
        trace.push(format!("{op:?}"));
        let ctx = || format!("seed {seed}, trace {trace:?}, model {model:?}");
        model = match (op, model.clone()) {
            (Op::Pid, m) => {
                assert_eq!(svc.pid(), pid, "pid() changed; {}", ctx());
                m
            }
            (Op::Signal(s), Model::Running) => {
                assert_eq!(
                    svc.signal(s),
                    Ok(()),
                    "B-M0-030: signal while running; {}",
                    ctx()
                );
                Model::Dying(vec![signum(s)])
            }
            (Op::Signal(s), Model::Dying(mut sent)) => {
                let r = svc.signal(s);
                assert!(
                    r == Ok(()) || r == Err(SignalError::Os { errno: 3 }),
                    "signal before reap must not report Exited: {r:?}; {}",
                    ctx()
                );
                sent.push(signum(s));
                Model::Dying(sent)
            }
            (Op::Signal(s), Model::Reaped(t)) => {
                assert_eq!(
                    svc.signal(s),
                    Err(SignalError::Exited),
                    "B-M0-030/I-1: signal after reap; {}",
                    ctx()
                );
                Model::Reaped(t)
            }
            (Op::Wait, Model::Dying(sent)) => {
                let r = svc.wait();
                let t = match r {
                    Ok(Termination::Signaled(n)) if sent.contains(&n) => Termination::Signaled(n),
                    other => panic!(
                        "B-M0-029/030: wait gave {other:?}, expected Signaled(one of {sent:?}); {}",
                        ctx()
                    ),
                };
                Model::Reaped(t)
            }
            (Op::Wait, Model::Reaped(t)) => {
                assert_eq!(
                    svc.wait(),
                    Ok(t),
                    "B-M0-029: repeated wait returns the same result; {}",
                    ctx()
                );
                Model::Reaped(t)
            }
            (Op::Stop, Model::Running) => {
                let r = svc.stop(Duration::from_secs(5));
                let want = StopOutcome {
                    termination: Termination::Signaled(15),
                    escalated: false,
                };
                assert_eq!(r, Ok(want), "B-M0-031: stop of sleep; {}", ctx());
                Model::Reaped(Termination::Signaled(15))
            }
            (op, m) => unreachable!("generator produced {op:?} in {m:?}"),
        };
        match &model {
            Model::Running => assert!(
                is_live(pid, st),
                "service must still be alive; seed {seed}, trace {trace:?}"
            ),
            Model::Reaped(_) => assert!(
                !same_process_exists(pid, st),
                "B-M0-029: /proc/{pid} still refers to the reaped process; seed {seed}, trace {trace:?}"
            ),
            Model::Dying(_) => {}
        }
    }
    // Leave nothing behind, and check the model's final transition too.
    match model {
        Model::Running => {
            assert_eq!(
                svc.signal(Signal::Kill),
                Ok(()),
                "seed {seed}: cleanup signal"
            );
            assert_eq!(
                svc.wait(),
                Ok(Termination::Signaled(9)),
                "seed {seed}: cleanup wait"
            );
        }
        Model::Dying(sent) => {
            let r = svc.wait();
            assert!(
                matches!(r, Ok(Termination::Signaled(n)) if sent.contains(&n)),
                "seed {seed}: final wait {r:?}, sent {sent:?}, trace {trace:?}"
            );
        }
        Model::Reaped(_) => {}
    }
    assert!(
        !same_process_exists(pid, st),
        "seed {seed}: process not reaped at the end"
    );
}

// spec: B-M0-029, B-M0-030, B-M0-031, I-1 (state-machine model, 24 fixed seeds x 8 steps)
#[test]
fn b_m0_029_030_031_service_state_machine_model() {
    require_prereqs();
    let _g = child_lock_shared();
    for case in 0..24u64 {
        // Distinct non-zero seeds: 0x9E3779B97F4A7C15 is the 64-bit golden-ratio constant.
        let seed = (case + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        run_case(seed, 8);
    }
}
