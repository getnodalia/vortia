//! The report of a section analysis, as the file an engineer keeps or sends: JSON for other
//! programs, CSV for a spreadsheet and a self-contained HTML page for reading.

use serde_json::{Value, json};

use crate::seal::seal;
use crate::section::{Density, SectionAnalysis, SectionOptions};

/// What a saved section report calls itself.
pub const SECTION_SCHEMA: &str = "vortia.section-report";

/// A section analysis with what it was made from.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionReport {
    pub label: String,
    pub case: String,
    pub time: String,
    pub options: SectionOptions,
    pub analysis: SectionAnalysis,
}

/// One number of the report: the key it has in JSON and CSV, its title and its unit.
pub struct Metric {
    pub key: &'static str,
    pub title: &'static str,
    pub unit: &'static str,
    pub value: f64,
}

/// The longest label kept.
const MAX_LABEL: usize = 160;

impl SectionReport {
    pub fn metrics(&self) -> Vec<Metric> {
        let a = &self.analysis;
        let m = |key, title, unit, value| Metric {
            key,
            title,
            unit,
            value,
        };
        vec![
            m("area", "Section area", "m²", a.area),
            m("massFlow", "Signed mass flow", "kg/s", a.mass_flow),
            m(
                "forwardMassFlow",
                "Forward mass flow",
                "kg/s",
                a.forward_mass_flow,
            ),
            m(
                "reverseMassFlow",
                "Reverse mass flow",
                "kg/s",
                a.reverse_mass_flow,
            ),
            m("volumeFlow", "Signed volume flow", "m³/s", a.volume_flow),
            m("meanPressure", "Area-mean pressure", "Pa", a.mean_pressure),
            m("meanDensity", "Area-mean density", "kg/m³", a.mean_density),
            m(
                "meanNormalVelocity",
                "Area-mean normal speed",
                "m/s",
                a.mean_normal_velocity,
            ),
            m("momentumFlux", "Normal momentum flux", "N", a.momentum_flux),
            m("pressureLoad", "Pressure term", "N", a.pressure_load),
            m("resultant", "Normal section resultant", "N", a.resultant),
            m("kineticPower", "Signed kinetic power", "W", a.kinetic_power),
            m(
                "coefficient",
                "Normal section coefficient",
                "",
                a.coefficient,
            ),
            m("meanCp", "Mean pressure coefficient", "", a.mean_cp),
        ]
    }

    fn label(&self) -> String {
        self.label.trim().chars().take(MAX_LABEL).collect()
    }

    fn options_json(&self) -> Value {
        let o = &self.options;
        json!({
            "axis": axis_name(o.axis),
            "direction": o.direction,
            "position": o.position,
            "density": match o.density {
                Density::Field => json!("field"),
                Density::Constant(c) => json!(c),
            },
            "kinematicPressure": o.kinematic_pressure,
            "referencePressure": o.reference_pressure,
            "referenceDensity": o.reference_density,
            "referenceSpeed": o.reference_speed,
            "referenceArea": o.reference_area,
            "wedgeAngle": o.wedge_angle,
        })
    }

    /// The report as JSON, in the envelope of [`seal`], with its digest.
    pub fn json(&self) -> String {
        let metrics: serde_json::Map<String, Value> = self
            .metrics()
            .into_iter()
            .map(|m| (m.key.to_owned(), json!(m.value)))
            .collect();
        let a = &self.analysis;
        seal(
            SECTION_SCHEMA,
            json!({
                "label": self.label(),
                "case": self.case,
                "time": self.time,
                "options": self.options_json(),
                "metrics": metrics,
                "cells": a.cells,
                "sharedFaces": a.shared_faces,
                "areaScale": a.area_scale,
                "assessment": "stored fields only; convergence and physical validity are not established by this report",
            }),
        )
    }

    /// The report as CSV, one row per number. A text that starts like a formula is quoted so a
    /// spreadsheet does not run it.
    pub fn csv(&self) -> String {
        let mut out = String::from("label,case,time,metric,value,unit\n");
        for m in self.metrics() {
            out.push_str(&format!(
                "{},{},{},{},{},{}\n",
                csv_text(&self.label()),
                csv_text(&self.case),
                csv_text(&self.time),
                m.key,
                m.value,
                csv_text(m.unit),
            ));
        }
        out
    }

    /// The report as a page that needs nothing else to open.
    pub fn html(&self) -> String {
        let mut rows = String::new();
        for m in self.metrics() {
            rows.push_str(&format!(
                "<tr><td>{}</td><td class=\"n\">{}</td><td>{}</td></tr>\n",
                escape(m.title),
                m.value,
                escape(m.unit)
            ));
        }
        let o = &self.options;
        let density = match o.density {
            Density::Field => "from the rho field".to_owned(),
            Density::Constant(c) => format!("{c} kg/m³"),
        };
        format!(
            "<!doctype html>\n<html lang=\"en\"><meta charset=\"utf-8\"><title>{title}</title>\n\
             <style>body{{font:15px system-ui,sans-serif;max-width:900px;margin:40px auto;padding:0 24px;color:#25313e}}\
             table{{border-collapse:collapse;width:100%;margin:20px 0}}td,th{{border-bottom:1px solid #d9dfe6;padding:8px 10px;text-align:left}}\
             td.n{{font-family:ui-monospace,monospace;text-align:right}}h1{{color:#177886}}small{{color:#5b6672}}</style>\n\
             <h1>{title}</h1>\n<p>{case} · time {time}</p>\n\
             <p>Section perpendicular to {axis} at {position}, counted {way}. Density {density}; \
             pressure {kin}; reference {pref} Pa, {rho} kg/m³, {speed} m/s, {area} m².</p>\n\
             <p><small>The resultant combines the normal momentum flux and the pressure term through the section. \
             It is not an engine thrust or a complete force on a body, and it does not show that the run converged \
             or that the model suits the flow.</small></p>\n\
             <table><tr><th>Quantity</th><th>Value</th><th>Unit</th></tr>\n{rows}</table>\n\
             <p><small>{cells} cells cut, {shared} shared faces counted once, scale {scale}.</small></p>\n</html>\n",
            title = escape(&self.label()),
            case = escape(&self.case),
            time = escape(&self.time),
            axis = axis_name(o.axis),
            position = o.position,
            way = if o.direction < 0.0 {
                "positive against the axis"
            } else {
                "positive along the axis"
            },
            kin = if o.kinematic_pressure {
                "is p/ρ"
            } else {
                "in Pa"
            },
            pref = o.reference_pressure,
            rho = o.reference_density,
            speed = o.reference_speed,
            area = o.reference_area,
            cells = self.analysis.cells,
            shared = self.analysis.shared_faces,
            scale = self.analysis.area_scale,
        )
    }
}

fn axis_name(axis: usize) -> &'static str {
    ["x", "y", "z"].get(axis).copied().unwrap_or("x")
}

pub(crate) fn escape(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '&' => "&amp;".to_owned(),
            '<' => "&lt;".to_owned(),
            '>' => "&gt;".to_owned(),
            '"' => "&quot;".to_owned(),
            '\'' => "&#39;".to_owned(),
            c => c.to_string(),
        })
        .collect()
}

pub(crate) fn csv_text(text: &str) -> String {
    let mut text = text.replace('"', "\"\"");
    if text
        .trim_start()
        .starts_with(['=', '+', '-', '@', '\t', '\r'])
    {
        text.insert(0, '\'');
    }
    format!("\"{text}\"")
}
