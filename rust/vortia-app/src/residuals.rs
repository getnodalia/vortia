//! The residuals of a run, read from the solver's log: the initial residual each equation
//! started every step with (`Solving for Ux, Initial residual = 1e-3, ...`), by field, so the
//! Run tab can draw how the run is converging.

/// How many fields are followed; a case solves a handful of equations.
const MAX_FIELDS: usize = 16;
/// How many steps are kept for each field; older ones are thinned out of the plot, not the log.
const MAX_POINTS: usize = 20_000;

/// The initial residuals of one field, by step.
#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    pub field: String,
    /// `(step, initial residual)`, in step order. Residuals that are not positive numbers are
    /// not kept: they have no place on a logarithmic axis.
    pub points: Vec<(f64, f64)>,
}

/// What the run's log has said about the residuals so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Residuals {
    pub series: Vec<Series>,
    step: Option<f64>,
    /// The first field whose residual was not a number or was infinite, and the step.
    non_finite: Option<(String, f64)>,
}

/// Why the residuals say a run is going wrong, for a warning while it runs.
#[derive(Clone, Debug, PartialEq)]
pub enum Trouble {
    /// A residual that is not a number or infinite: the fields are no longer valid.
    NotANumber { field: String, step: f64 },
    /// A residual that fell and then went back up to where it started or far above it.
    Growing {
        field: String,
        step: f64,
        residual: f64,
        lowest: f64,
    },
}

/// Fewest steps of a field before its growth is judged: the first steps of a run move a lot.
const JUDGED_AFTER: usize = 10;
/// A residual this high is a diverging run whatever it was before.
const ABSURD: f64 = 1.0e6;
/// How far above its lowest value a residual that is back at the size it started with is.
const RISE: f64 = 1.0e3;

impl Residuals {
    /// Takes one line of the log; lines that say nothing about residuals change nothing.
    pub fn push(&mut self, line: &str) {
        if let Some(rest) = line.trim().strip_prefix("Time =") {
            self.step = rest.trim().parse::<f64>().ok().filter(|t| t.is_finite());
            return;
        }
        let Some((field, residual)) = parse_solve(line) else {
            return;
        };
        if !residual.is_finite() && self.non_finite.is_none() {
            self.non_finite = Some((field.to_owned(), self.step.unwrap_or(0.0)));
        }
        let (Some(step), true) = (self.step, residual > 0.0 && residual.is_finite()) else {
            return;
        };
        let index = match self.series.iter().position(|s| s.field == field) {
            Some(i) => i,
            None if self.series.len() < MAX_FIELDS => {
                self.series.push(Series {
                    field: field.to_owned(),
                    points: Vec::new(),
                });
                self.series.len() - 1
            }
            None => return,
        };
        if let Some(series) = self.series.get_mut(index) {
            if series.points.len() >= MAX_POINTS {
                // Keep every other point: the shape of the whole run survives.
                let mut keep = false;
                series.points.retain(|_| {
                    keep = !keep;
                    keep
                });
            }
            // A field solved more than once in a step (a corrector) keeps its first residual.
            if series.points.last().is_none_or(|(t, _)| *t < step) {
                series.points.push((step, residual));
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.series.is_empty()
    }

    /// What the residuals say is going wrong, if anything. It is a warning, not a verdict: a
    /// residual can climb for a while in a healthy run, so it asks for more than a bump.
    pub fn trouble(&self) -> Option<Trouble> {
        if let Some((field, step)) = &self.non_finite {
            return Some(Trouble::NotANumber {
                field: field.clone(),
                step: *step,
            });
        }
        self.series.iter().find_map(|s| {
            let (step, residual) = *s.points.last()?;
            let lowest = s
                .points
                .iter()
                .map(|(_, r)| *r)
                .fold(f64::INFINITY, f64::min);
            let grown =
                s.points.len() >= JUDGED_AFTER && residual >= 1.0 && residual >= lowest * RISE;
            (grown || residual >= ABSURD).then(|| Trouble::Growing {
                field: s.field.clone(),
                step,
                residual,
                lowest,
            })
        })
    }

    /// The residuals as a table: one row per step, one column per field, empty where a field
    /// was not solved in that step. A field name that could break the header is not a name
    /// the solver prints, but it comes from a log, so commas and line breaks are made spaces.
    pub fn csv(&self) -> String {
        let mut steps: Vec<f64> = self
            .series
            .iter()
            .flat_map(|s| s.points.iter().map(|(t, _)| *t))
            .collect();
        steps.sort_by(f64::total_cmp);
        steps.dedup();
        let mut text = String::from("step");
        for s in &self.series {
            text.push(',');
            text.extend(s.field.chars().map(|c| {
                if matches!(c, ',' | '\n' | '\r' | '"') {
                    ' '
                } else {
                    c
                }
            }));
        }
        text.push('\n');
        for step in steps {
            text.push_str(&step.to_string());
            for s in &self.series {
                text.push(',');
                if let Ok(i) = s.points.binary_search_by(|(t, _)| t.total_cmp(&step)) {
                    if let Some((_, r)) = s.points.get(i) {
                        text.push_str(&format!("{r:e}"));
                    }
                }
            }
            text.push('\n');
        }
        text
    }

    /// The step range and the residual range of everything kept.
    pub fn bounds(&self) -> Option<([f64; 2], [f64; 2])> {
        let mut points = self.series.iter().flat_map(|s| s.points.iter());
        let first = points.next()?;
        let mut b = ([first.0, first.0], [first.1, first.1]);
        for (t, r) in points {
            b.0 = [b.0[0].min(*t), b.0[1].max(*t)];
            b.1 = [b.1[0].min(*r), b.1[1].max(*r)];
        }
        Some(b)
    }
}

/// `"smoothSolver:  Solving for Ux, Initial residual = 1e0, Final residual = ..."` gives
/// `("Ux", 1.0)`.
fn parse_solve(line: &str) -> Option<(&str, f64)> {
    let rest = line.split_once("Solving for ")?.1;
    let (field, rest) = rest.split_once(',')?;
    let value = rest.trim().strip_prefix("Initial residual =")?;
    let value = value.split(',').next()?.trim().parse::<f64>().ok()?;
    let field = field.trim();
    (!field.is_empty()).then_some((field, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(lines: &[&str]) -> Residuals {
        let mut r = Residuals::default();
        for l in lines {
            r.push(l);
        }
        r
    }

    #[test]
    fn the_table_has_a_column_per_field_and_a_row_per_step() {
        let r = feed(&[
            "Time = 1",
            "smoothSolver:  Solving for Ux, Initial residual = 1e0, Final residual = 8.8e-2, No Iterations 1",
            "Time = 2",
            "smoothSolver:  Solving for Ux, Initial residual = 2.5e-1, Final residual = 6.6e-3, No Iterations 2",
            "DICPCG:  Solving for p, Initial residual = 1.8e-2, Final residual = 6.5e-4, No Iterations 65",
        ]);
        assert_eq!(r.csv(), "step,Ux,p\n1,1e0,\n2,2.5e-1,1.8e-2\n");
        assert_eq!(Residuals::default().csv(), "step\n");
    }

    #[test]
    fn the_initial_residual_of_each_field_is_followed_step_by_step() {
        let r = feed(&[
            "Time = 1",
            "",
            "smoothSolver:  Solving for Ux, Initial residual = 1e0, Final residual = 8.8e-2, No Iterations 1",
            "DICPCG:  Solving for p, Initial residual = 9.9e-1, Final residual = 4.5e-2, No Iterations 65",
            "Time = 2",
            "smoothSolver:  Solving for Ux, Initial residual = 2.5e-1, Final residual = 6.6e-3, No Iterations 2",
            "DICPCG:  Solving for p, Initial residual = 1.8e-2, Final residual = 6.5e-4, No Iterations 65",
        ]);
        let names: Vec<&str> = r.series.iter().map(|s| s.field.as_str()).collect();
        assert_eq!(names, ["Ux", "p"]);
        assert_eq!(r.series[0].points, [(1.0, 1.0), (2.0, 0.25)]);
        assert_eq!(r.series[1].points, [(1.0, 0.99), (2.0, 0.018)]);
        assert_eq!(r.bounds(), Some(([1.0, 2.0], [0.018, 1.0])));
    }

    #[test]
    fn a_corrector_in_the_same_step_does_not_add_a_point() {
        let r = feed(&[
            "Time = 1",
            "DICPCG:  Solving for p, Initial residual = 1e0, Final residual = 1e-2, No Iterations 4",
            "DICPCG:  Solving for p, Initial residual = 1e-2, Final residual = 1e-4, No Iterations 4",
        ]);
        assert_eq!(r.series[0].points, [(1.0, 1.0)]);
    }

    #[test]
    fn lines_that_are_not_residuals_or_are_not_numbers_change_nothing() {
        let r = feed(&[
            "Solving for Ux, Initial residual = 1e0, Final residual = 1e-2, No Iterations 4",
            "Time = soon",
            "Solving for p, Initial residual = 1e0, Final residual = 1e-2, No Iterations 4",
            "Time = 1",
            "Solving for p, Initial residual = nan, Final residual = 1e-2, No Iterations 4",
            "Solving for p, Initial residual = 0, Final residual = 0, No Iterations 0",
            "Solving for , Initial residual = 1",
            "ExecutionTime = 3 s  ClockTime = 3 s",
            "End",
        ]);
        assert!(r.is_empty(), "{r:?}");
        assert_eq!(r.bounds(), None);
    }

    fn run(field: &str, residuals: &[f64]) -> Residuals {
        let mut r = Residuals::default();
        for (t, x) in residuals.iter().enumerate() {
            r.push(&format!("Time = {}", t + 1));
            r.push(&format!(
                "Solving for {field}, Initial residual = {x:e}, Final residual = 1e-5, No Iterations 1"
            ));
        }
        r
    }

    #[test]
    fn a_run_that_converges_or_bumps_is_not_warned_about() {
        let falling: Vec<f64> = (0..40).map(|i| 10f64.powf(-f64::from(i) / 5.0)).collect();
        assert_eq!(run("p", &falling).trouble(), None);
        // A rise of a few decades that stays small is an ordinary transient.
        let mut bump = falling.clone();
        bump.extend([1e-3, 1e-2, 2e-2]);
        assert_eq!(run("p", &bump).trouble(), None);
        // Too few steps to judge: the first steps of a run jump about.
        assert_eq!(run("p", &[1e-6, 5.0]).trouble(), None);
    }

    #[test]
    fn a_residual_back_at_the_start_or_far_above_it_is_a_warning() {
        let mut growing: Vec<f64> = (0..20).map(|i| 10f64.powf(-f64::from(i) / 4.0)).collect();
        growing.extend([2.0, 40.0]);
        let Some(Trouble::Growing {
            field,
            step,
            residual,
            lowest,
        }) = run("Ux", &growing).trouble()
        else {
            panic!("a run that went from 1e-5 to 40 is diverging");
        };
        assert_eq!((field.as_str(), step, residual), ("Ux", 22.0, 40.0));
        assert!(lowest < 1e-4);
        // Whatever it was before, a huge residual is a diverging run.
        assert!(matches!(
            run("p", &[1e7]).trouble(),
            Some(Trouble::Growing { .. })
        ));
    }

    #[test]
    fn a_residual_that_is_not_a_number_is_a_warning_with_where_it_was_first_seen() {
        let r = feed(&[
            "Time = 7",
            "Solving for p, Initial residual = 1e-1, Final residual = 1e-2, No Iterations 4",
            "Time = 8",
            "Solving for T, Initial residual = nan, Final residual = 1e-2, No Iterations 4",
            "Time = 9",
            "Solving for k, Initial residual = inf, Final residual = 1e-2, No Iterations 4",
        ]);
        assert_eq!(
            r.trouble(),
            Some(Trouble::NotANumber {
                field: "T".into(),
                step: 8.0
            })
        );
    }

    #[test]
    fn the_fields_and_points_followed_are_bounded() {
        let mut r = Residuals::default();
        for t in 0..MAX_POINTS + 10 {
            r.push(&format!("Time = {t}"));
            r.push(
                "Solving for p, Initial residual = 1e-3, Final residual = 1e-5, No Iterations 1",
            );
        }
        assert!(r.series[0].points.len() <= MAX_POINTS);
        r.push("Time = 99999999");
        for n in 0..MAX_FIELDS + 5 {
            r.push(&format!(
                "Solving for f{n}, Initial residual = 1e-3, Final residual = 1e-5, No Iterations 1"
            ));
        }
        assert_eq!(r.series.len(), MAX_FIELDS);
    }
}
