//! Observabilidade (Fase 9): todo evento sai como log JSON (Workers Logs
//! indexa os campos) e como ponto no Analytics Engine (binding `METRICS`).
//!
//! Formato do log: `{"level": "info|warn|error", "event": "...", ...campos}`.
//! Ponto de métrica: índice = evento; blobs = [evento, outcome, domínio];
//! doubles = [tamanho em bytes, contagem]. Sem endereços de email na
//! métrica: só o domínio.

use serde_json::{json, Value};
use worker::{console_error, console_log, console_warn, AnalyticsEngineDataPointBuilder, Env};

#[derive(Clone, Copy)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// Log + métrica.
pub fn event(env: &Env, level: Level, name: &str, fields: Value) {
    log(level, name, &fields);
    metric(env, name, level, &fields);
}

/// Só log (sem `Env`, ex.: antes dos bindings existirem).
pub fn log(level: Level, name: &str, fields: &Value) {
    let mut entry = json!({ "level": level.as_str(), "event": name });
    if let (Some(e), Some(f)) = (entry.as_object_mut(), fields.as_object()) {
        for (k, v) in f {
            e.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    match level {
        Level::Info => console_log!("{entry}"),
        Level::Warn => console_warn!("{entry}"),
        Level::Error => console_error!("{entry}"),
    }
}

/// Só métrica. Sem o binding (ex.: dev local) ou se a escrita falhar, nada
/// acontece: métrica nunca derruba o fluxo principal.
pub fn metric(env: &Env, name: &str, level: Level, fields: &Value) {
    let Ok(ds) = env.analytics_engine("METRICS") else {
        return;
    };
    let str_field = |k: &str| {
        fields
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let num_field =
        |k: &str, default: f64| fields.get(k).and_then(Value::as_f64).unwrap_or(default);
    let outcome = match str_field("outcome") {
        o if o.is_empty() => level.as_str().to_owned(),
        o => o,
    };
    let point = AnalyticsEngineDataPointBuilder::new()
        .indexes([name])
        .blobs([name.to_owned(), outcome, str_field("domain")])
        .doubles([num_field("size", 0.0), num_field("count", 1.0)])
        .build();
    let _ = ds.write_data_point(&point);
}
