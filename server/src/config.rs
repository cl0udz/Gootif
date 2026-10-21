use std::collections::HashMap;
use std::{env, fs};

/// KEY=VALUE file (one per line, value taken verbatim after the first '=').
/// Lookup order matches the PHP app: config value, then environment, then default.
pub struct Config {
    values: HashMap<String, String>,
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let path = env::var("GOOTIF_CONFIG").unwrap_or_else(|_| "/etc/gootif/gootif.env".into());
        let text = fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
        let values = text
            .lines()
            .map(|l| l.trim_end_matches('\r'))
            .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.to_string()))
            .collect();
        Ok(Self { values })
    }

    #[cfg(test)]
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        Self { values: pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
    }

    pub fn option(&self, name: &str, default: &str) -> String {
        if let Some(v) = self.values.get(name) {
            return v.clone();
        }
        env::var(name).unwrap_or_else(|_| default.to_string())
    }

    pub fn require(&self, name: &str) -> Result<String, String> {
        let v = self.option(name, "");
        if v.is_empty() {
            Err(format!("{name} is not configured"))
        } else {
            Ok(v)
        }
    }
}
