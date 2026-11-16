//! Learning from the person's own choices (the Power Daemon design): when
//! the same mode is picked by hand in the same situation a few times, offer
//! to make it automatic. Never applied without a yes.
//!
//! - **An app:** the same mode picked three times while the same app was in
//!   front ("You chose Faster three times while using Blender").
//! - **A time of day:** the same mode picked around the same hour on three
//!   different days ("You chose Longer battery around 21:00 on three
//!   days").
//!
//! One suggestion at a time; a "no" is remembered and not asked again.

use std::path::Path;

use dynamis_core::auto::{AppRule, Prefs, TimeRule};
use dynamis_core::profiles::Profile;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::write_atomic;

const FILE: &str = "choices.json";
const VERSION: u32 = 1;
const KEEP: usize = 200;
/// Choices needed before suggesting.
const ENOUGH: usize = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub time: f64,
    pub profile: Profile,
    pub app: Option<String>,
    pub hour: Option<u8>,
    pub day: Option<i64>,
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    v: u32,
    choices: Vec<Choice>,
    /// Suggestion ids the person said no to.
    #[serde(default)]
    declined: Vec<String>,
}

#[derive(Default)]
pub struct Habits {
    choices: Vec<Choice>,
    declined: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Suggestion {
    App {
        id: String,
        app: String,
        profile: Profile,
        count: usize,
    },
    Time {
        id: String,
        from_hour: u8,
        to_hour: u8,
        profile: Profile,
        days: usize,
    },
}

impl Suggestion {
    pub fn id(&self) -> &str {
        match self {
            Suggestion::App { id, .. } | Suggestion::Time { id, .. } => id,
        }
    }
}

impl Habits {
    pub fn load(dir: &Path) -> Habits {
        let s = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Saved>(&t).ok())
            .unwrap_or_default();
        Habits {
            choices: s.choices,
            declined: s.declined,
        }
    }

    fn save(&self, dir: &Path) {
        if let Ok(t) = serde_json::to_string(&Saved {
            v: VERSION,
            choices: self.choices.clone(),
            declined: self.declined.clone(),
        }) {
            let _ = write_atomic(&dir.join(FILE), &t);
        }
    }

    /// The person picked a mode by hand.
    pub fn note(&mut self, dir: &Path, c: Choice) {
        self.choices.push(c);
        if self.choices.len() > KEEP {
            let extra = self.choices.len() - KEEP;
            self.choices.drain(..extra);
        }
        self.save(dir);
    }

    /// The suggestion to show now, if any (not already a rule, not
    /// declined).
    pub fn suggestion(&self, prefs: &Prefs) -> Option<Suggestion> {
        // An app: the latest choices first.
        let mut apps: Vec<(String, Profile, usize)> = Vec::new();
        for c in self.choices.iter().rev() {
            let Some(app) = &c.app else { continue };
            match apps
                .iter_mut()
                .find(|(a, p, _)| a == app && *p == c.profile)
            {
                Some(e) => e.2 += 1,
                None => apps.push((app.clone(), c.profile, 1)),
            }
        }
        for (app, profile, count) in apps {
            let id = format!("app:{app}:{}", profile.as_str());
            if count >= ENOUGH
                && !self.declined.contains(&id)
                && !prefs.app_rules.iter().any(|r| r.app == app)
            {
                return Some(Suggestion::App {
                    id,
                    app,
                    profile,
                    count,
                });
            }
        }
        // A time of day: the same mode around the same hour on 3+ days.
        for h in 0..24u8 {
            for profile in Profile::ALL {
                let mut days: Vec<i64> = self
                    .choices
                    .iter()
                    .filter(|c| c.profile == profile && c.app.is_none())
                    .filter(|c| c.hour.is_some_and(|ch| ch == h || ch == (h + 1) % 24))
                    .filter_map(|c| c.day)
                    .collect();
                days.sort_unstable();
                days.dedup();
                // The window starts at a chosen hour, not the hour before.
                let starts_here = self
                    .choices
                    .iter()
                    .any(|c| c.profile == profile && c.app.is_none() && c.hour == Some(h));
                if !starts_here {
                    continue;
                }
                let id = format!("time:{h}:{}", profile.as_str());
                // Windows overlap by an hour: a no also covers the
                // neighbouring hours, so the same habit is not asked again.
                let near_declined = [23, 0, 1].iter().any(|d| {
                    self.declined
                        .contains(&format!("time:{}:{}", (h + d) % 24, profile.as_str()))
                });
                if days.len() >= ENOUGH
                    && !near_declined
                    && !prefs.time_rules.iter().any(|r| r.matches(h))
                {
                    return Some(Suggestion::Time {
                        id,
                        from_hour: h,
                        to_hour: (h + 2) % 24,
                        profile,
                        days: days.len(),
                    });
                }
            }
        }
        None
    }

    /// The answer: `Some(prefs)` to save if yes, `None` if no (remembered).
    pub fn answer(
        &mut self,
        dir: &Path,
        prefs: &Prefs,
        id: &str,
        yes: bool,
    ) -> Result<Option<Prefs>, String> {
        let s = self
            .suggestion(prefs)
            .filter(|s| s.id() == id)
            .ok_or("that suggestion is no longer open")?;
        if !yes {
            self.declined.push(id.to_string());
            self.save(dir);
            return Ok(None);
        }
        let mut p = prefs.clone();
        match s {
            Suggestion::App { app, profile, .. } => p.app_rules.push(AppRule { app, profile }),
            Suggestion::Time {
                from_hour,
                to_hour,
                profile,
                ..
            } => p.time_rules.push(TimeRule {
                from_hour,
                to_hour,
                profile,
            }),
        }
        Ok(Some(p))
    }

    pub fn json(&self, prefs: &Prefs) -> Value {
        json!(self.suggestion(prefs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("dynamis-habits-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn three_times_with_the_same_app_suggests_a_rule() {
        let d = dir("app");
        let mut h = Habits::load(&d);
        let prefs = Prefs::default();
        for i in 0..2 {
            h.note(
                &d,
                Choice {
                    time: i as f64,
                    profile: Profile::Performance,
                    app: Some("blender".into()),
                    hour: Some(10),
                    day: Some(1),
                },
            );
        }
        assert!(h.suggestion(&prefs).is_none());
        h.note(
            &d,
            Choice {
                time: 3.0,
                profile: Profile::Performance,
                app: Some("blender".into()),
                hour: Some(10),
                day: Some(1),
            },
        );
        let s = h.suggestion(&prefs).unwrap();
        assert!(
            matches!(&s, Suggestion::App { app, profile: Profile::Performance, .. } if app == "blender")
        );
        let p = h.answer(&d, &prefs, s.id(), true).unwrap().unwrap();
        assert_eq!(p.app_rules[0].app, "blender");
        assert!(h.suggestion(&p).is_none(), "already a rule");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_same_hour_on_three_days_suggests_a_time_rule_and_no_is_remembered() {
        let d = dir("time");
        let mut h = Habits::load(&d);
        let prefs = Prefs::default();
        for day in 0..3 {
            h.note(
                &d,
                Choice {
                    time: day as f64,
                    profile: Profile::PowerSaver,
                    app: None,
                    hour: Some(21),
                    day: Some(day),
                },
            );
        }
        let s = h.suggestion(&prefs).unwrap();
        assert!(matches!(
            s,
            Suggestion::Time {
                profile: Profile::PowerSaver,
                days: 3,
                ..
            }
        ));
        assert!(h.answer(&d, &prefs, s.id(), false).unwrap().is_none());
        assert!(
            Habits::load(&d).suggestion(&prefs).is_none(),
            "no is remembered"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
