//! PFML3 prompt builder for PLaMo 3 Translate (format from the official model card).
use serde::{Deserialize, Serialize};

pub const STOP: [&str; 2] = ["<|plamo:tag|>", "<|plamo:bos|>"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Form {
    Plain,
    Polite,
}

#[derive(Debug, Clone, Default)]
pub struct TranslateRequest<'a> {
    pub text: &'a str,
    pub source_lang: Option<&'a str>,
    pub target_lang: &'a str,
    pub form: Option<Form>,
    pub context: Option<&'a str>,
    pub history: &'a [(String, String)],
    pub glossary: &'a [(String, String)],
}

fn tag(out: &mut String, name: &str, attrs: &[(&str, &str)], msg: &str) {
    out.push_str("<|plamo:tag|>");
    out.push_str(name);
    for (k, v) in attrs {
        out.push_str(&format!("<|plamo:key|>{k}<|plamo:val|>{v}"));
    }
    out.push_str("<|plamo:msg|>");
    out.push_str(msg);
}

pub fn build(r: &TranslateRequest) -> String {
    let mut p = String::new();
    tag(&mut p, "dataset", &[], "translation");
    if let Some(c) = r.context.filter(|c| !c.is_empty()) {
        tag(&mut p, "context", &[], c);
    }
    let src: Vec<(&str, &str)> = r.source_lang.map(|l| ("lang", l)).into_iter().collect();
    let mut dst = vec![("lang", r.target_lang)];
    if let Some(f) = r.form {
        dst.push(("form", if f == Form::Plain { "plain" } else { "polite" }));
    }
    for (s, t) in r.history {
        tag(&mut p, "input", &src, s);
        tag(&mut p, "output", &dst, t);
    }
    if !r.glossary.is_empty() {
        let g: String = r.glossary.iter().map(|(s, t)| format!("<|plamo:index|>{s}<|plamo:item|>{t}")).collect();
        tag(&mut p, "glossary", &[], &g);
    }
    tag(&mut p, "input", &src, r.text);
    tag(&mut p, "output", &dst, "");
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_model_card_example() {
        let p = build(&TranslateRequest { text: "会議は来週の月曜日に変更されました。", target_lang: "en", ..Default::default() });
        assert_eq!(
            p,
            "<|plamo:tag|>dataset<|plamo:msg|>translation<|plamo:tag|>input<|plamo:msg|>会議は来週の月曜日に変更されました。<|plamo:tag|>output<|plamo:key|>lang<|plamo:val|>en<|plamo:msg|>"
        );
    }

    #[test]
    fn orders_context_history_glossary_input() {
        let h = [("a".to_string(), "A".to_string())];
        let g = [("Foo".to_string(), "フー".to_string())];
        let p = build(&TranslateRequest {
            text: "b",
            target_lang: "ja",
            form: Some(Form::Polite),
            context: Some("ctx"),
            history: &h,
            glossary: &g,
            ..Default::default()
        });
        let at = |s: &str| p.find(s).unwrap();
        assert!(at("context") < at("<|plamo:msg|>a") && at("<|plamo:msg|>a") < at("glossary") && at("glossary") < at("<|plamo:msg|>b"));
        assert!(p.ends_with("<|plamo:key|>form<|plamo:val|>polite<|plamo:msg|>"));
    }
}
