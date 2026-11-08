// Copyright (c) 2020-2026  Brendan Molloy <brendan@bbqsrc.net>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::{cell::RefCell, iter};

use crate::{keywords::Keywords, tagexpr::TagOperation};
use crate::{
    Background, Docstring, Examples, Feature, LineCol, Rule, Scenario, Span, Step, StepType, Table,
};

#[derive(Debug)]
pub struct GherkinEnv {
    keywords: RefCell<Keywords<'static>>,
    pub(crate) last_error: RefCell<Option<EnvError>>,
    pub(crate) fatal_error: RefCell<Option<EnvError>>,
    fatal_error_offset: RefCell<Option<usize>>,
    last_step: RefCell<Option<StepType>>,
    last_keyword: RefCell<Option<String>>,
    line_offsets: RefCell<Vec<usize>>,
    was_escaped: RefCell<bool>,
}

#[derive(Debug, thiserror::Error)]
pub enum EnvError {
    #[error("Requested language '{0}' is not supported.")]
    UnsupportedLanguage(String),

    #[error("Unknown keyword: '{0}'.")]
    UnknownKeyword(String),

    #[error("Inconsistent cell count")]
    InconsistentCellCount(Vec<Vec<String>>),
}

impl GherkinEnv {
    pub fn new(language: &str) -> Result<Self, EnvError> {
        let keywords = Keywords::get(language)
            .ok_or_else(|| EnvError::UnsupportedLanguage(language.into()))?;

        Ok(Self {
            keywords: RefCell::new(keywords),
            ..Default::default()
        })
    }

    pub fn set_language(&self, language: &str) -> Result<(), &'static str> {
        self.set_language_at(language, None)
    }

    /// Same as [`GherkinEnv::set_language()`], but also records the `offset`
    /// of the language directive, to report an unsupported language at.
    fn set_language_at(&self, language: &str, offset: Option<usize>) -> Result<(), &'static str> {
        let keywords = Keywords::get(language).ok_or_else(|| {
            self.set_fatal_error(EnvError::UnsupportedLanguage(language.into()), offset);
            "Unsupported language"
        })?;

        *self.keywords.borrow_mut() = keywords;

        Ok(())
    }

    fn assert_no_error(&self) -> Result<(), &'static str> {
        if self.fatal_error.borrow().is_some() {
            return Err("fatal error");
        }

        Ok(())
    }

    /// Records the first fatal `error` along with the `offset` of the input
    /// it was found at, if known, which is reported as the error position
    /// instead of the one the parser failed at.
    fn set_fatal_error(&self, error: EnvError, offset: Option<usize>) {
        if self.fatal_error.borrow().is_some() {
            return;
        }

        *self.fatal_error.borrow_mut() = Some(error);
        *self.fatal_error_offset.borrow_mut() = offset;
    }

    fn set_last_error(&self, error: EnvError) {
        *self.last_error.borrow_mut() = Some(error);
    }

    fn keywords(&self) -> std::cell::Ref<'_, Keywords<'static>> {
        self.keywords.borrow()
    }

    fn set_keyword(&self, kw: String) {
        *self.last_keyword.borrow_mut() = Some(kw);
    }

    fn clear_keyword(&self) {
        *self.last_keyword.borrow_mut() = None;
    }

    fn last_keyword(&self) -> std::cell::Ref<'_, Option<String>> {
        self.last_keyword.borrow()
    }

    fn take_keyword(&self) -> String {
        self.last_keyword.borrow_mut().take().unwrap()
    }

    fn set_last_step(&self, ty: StepType) {
        *self.last_step.borrow_mut() = Some(ty);
    }

    fn clear_last_step(&self) {
        *self.last_step.borrow_mut() = None;
    }

    fn last_step(&self) -> Option<StepType> {
        *self.last_step.borrow()
    }

    fn increment_nl(&self, offset: usize) {
        let mut line_offsets = self.line_offsets.borrow_mut();
        if !line_offsets.contains(&offset) {
            line_offsets.push(offset);
        }
    }

    fn position(&self, offset: usize) -> LineCol {
        let line_offsets = self.line_offsets.borrow();
        let line = line_offsets
            .iter()
            .position(|x| x > &offset)
            .unwrap_or(line_offsets.len());

        let col = offset - line_offsets[line - 1] + 1;

        LineCol { line, col }
    }

    /// Returns the position to report for a failure to parse the `input`,
    /// given the `offset` the parser failed at.
    ///
    /// The `input` is what the parser read. It ends with a `\n`, and
    /// `appended_newline` tells whether the caller added that `\n` itself.
    ///
    /// Like the reference Gherkin parser, points to the first non-blank
    /// character of the line causing the failure (blank lines never cause
    /// one), or to column 0 of the line past the end of the `input`, if it
    /// ended unexpectedly. A fatal error points to where it was found instead.
    ///
    /// Like the reference too, a blank last line does not count when the
    /// source has no final newline, any leading whitespace counts as
    /// indentation (see [`is_js_whitespace()`]), and a column is counted in
    /// characters, starting from 1.
    pub(crate) fn error_position(
        &self,
        input: &str,
        appended_newline: bool,
        offset: usize,
    ) -> LineCol {
        let offset = self.fatal_error_offset.borrow().unwrap_or(offset);
        let first = input[..offset].matches('\n').count();

        let body = input.strip_suffix('\n').unwrap_or(input);
        let mut lines = body
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .collect::<Vec<_>>();
        if appended_newline && lines.last().is_some_and(|l| is_blank(l)) {
            lines.pop();
        }

        for (n, text) in lines.iter().enumerate().skip(first) {
            let trimmed = text.trim_start_matches(is_js_whitespace);
            if !trimmed.is_empty() {
                let indent = &text[..text.len() - trimmed.len()];
                let col = indent.chars().count() + 1;
                return LineCol { line: n + 1, col };
            }
        }
        LineCol {
            line: lines.len() + 1,
            col: 0,
        }
    }

    fn escaped(&self) -> bool {
        *self.was_escaped.borrow()
    }

    fn set_escaped(&self, v: bool) {
        *self.was_escaped.borrow_mut() = v;
    }
}

impl Default for GherkinEnv {
    fn default() -> Self {
        GherkinEnv {
            keywords: RefCell::new(Keywords::default()),
            last_error: RefCell::new(None),
            fatal_error: RefCell::new(None),
            fatal_error_offset: RefCell::new(None),
            last_step: RefCell::new(None),
            last_keyword: RefCell::new(None),
            line_offsets: RefCell::new(vec![0]),
            was_escaped: RefCell::new(false),
        }
    }
}

/// Checks whether the `c` is whitespace for the reference Gherkin parser.
///
/// That one is written in JavaScript, where `\s` matches U+FEFF (the byte
/// order mark) but not U+0085, the other way round from [`char::is_whitespace()`].
fn is_js_whitespace(c: char) -> bool {
    c == '\u{feff}' || (c.is_whitespace() && c != '\u{85}')
}

/// Checks whether the `line` holds nothing but whitespace, as the reference
/// Gherkin parser sees it.
fn is_blank(line: &str) -> bool {
    line.trim_matches(is_js_whitespace).is_empty()
}

/// Builds a [`Docstring`] from the `raw` text found between its `delimiter`s.
///
/// The rest of the opening delimiter's line is the media type. Every other line
/// has the opening delimiter's indentation removed (or all of its indentation,
/// if it has less), and escaped delimiters are unescaped.
fn docstring(delimiter: &str, raw: &str, span: Span, position: LineCol) -> Docstring {
    let indent = position.col - 1;
    let escaped = delimiter
        .chars()
        .map(|c| format!("\\{c}"))
        .collect::<String>();

    let mut lines = raw.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l));
    let media_type = lines
        .next()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(ToOwned::to_owned);
    let mut lines = lines.collect::<Vec<_>>();
    // The last line only holds the indentation of the closing delimiter.
    if lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }

    let content = lines
        .into_iter()
        .map(|line| {
            let trimmed = line.trim_start();
            let line = if line.chars().count() - trimmed.chars().count() < indent {
                trimmed
            } else {
                line.char_indices()
                    .nth(indent)
                    .map_or("", |(i, _)| &line[i..])
            };
            line.replace(&escaped, delimiter)
        })
        .collect::<Vec<_>>()
        .join("\n");

    Docstring::builder()
        .content(content)
        .media_type(media_type)
        .delimiter(delimiter.to_owned())
        .span(span)
        .position(position)
        .build()
}

peg::parser! { pub(crate) grammar gherkin_parser(env: &GherkinEnv) for str {

rule _() = quiet!{[' ' | '\t']*}
rule __() = quiet!{([' ' | '\t'] / nl())*}

rule nl0() = quiet!{"\r"? "\n"}
rule nl_no_comment() = quiet!{nl0() p:position!() {
    env.increment_nl(p);
}}
rule nl() = quiet!{comment_no_nl()? nl0() p:position!() {
    env.increment_nl(p);
}}
rule eof() = quiet!{![_]}
rule nl_eof() = quiet!{(nl() / [' ' | '\t'])+ / eof()}
rule comment_no_nl() = quiet!{[' ' | '\t']* "#" $((!nl0()[_])*)}
rule comment() = quiet!{comment_no_nl() nl_eof()}
rule not_nl() -> &'input str = n:$((!nl0()[_])+) { n }

rule keyword1(list: &[&str]) -> &'input str
    = input:$((!nl0() [_])+) {?
        // println!("Input: {}", &input);
        // The longest keyword wins, so `Regla` does not shadow `Regla de negocio`.
        if let Some(v) = list
            .iter()
            .filter(|x| input.starts_with(**x))
            .max_by_key(|x| x.chars().count())
        {
            env.set_keyword((*v).to_string());
            // println!("Found: {}", &v);
            Err("success")
        } else {
            // println!("Unfound: {}", &input);
            env.clear_keyword();
            env.set_last_error(EnvError::UnknownKeyword(
                input.trim_matches(is_js_whitespace).into(),
            ));
            Err("unknown keyword")
        }
    }

rule keyword0(list: &[&str]) -> usize
    = keyword1(list)? {?
        match env.last_keyword().as_ref() {
            Some(v) => Ok(v.chars().count()),
            None => Err("no match")
        }
    }

pub(crate) rule keyword<'a>(list: &[&'a str]) -> &'a str
    = comment()* len:keyword0(list) [_]*<{len}> {
        let kw = env.take_keyword();
        list.iter().find(|x| **x == &*kw).unwrap()
    }

rule language_directive() -> ()
    = ([' ' | '\t'] / nl_no_comment())* p:position!()
      "#" _ "language" _ ":" _ l:$(not_nl()+) _ nl() {?
        env.set_language_at(l, Some(p))
    }

rule docstring() -> Docstring
    = pa:position!() "\"\"\"" n:$((!nl0() [_])* nl() (!"\"\"\"" (nl() / [_]))*) "\"\"\"" pb:position!() nl_eof() {
        docstring("\"\"\"", n, Span { start: pa, end: pb }, env.position(pa))
    }
    / pa:position!() "```" n:$((!nl0() [_])* nl() (!"```" (nl() / [_]))*) "```" pb:position!() nl_eof() {
        docstring("```", n, Span { start: pa, end: pb }, env.position(pa))
    }

rule escaped_cell_char() -> &'input str
    = "\\n" { "\n" } / "\\|" { "|" } / "\\\\" { "\\" }

rule table_cell() -> Vec<&'input str>
    = "|" _ !(nl0() / eof()) n:((escaped_cell_char() / $(!("|" / nl0())[_]))*) { n }

pub(crate) rule table_row() -> Vec<String>
    = n:(table_cell() ** _) _ "|" _ nl_eof() {
        n.into_iter()
            .map(|s| {
                let Some((start, first)) = s.iter().enumerate()
                    .find_map(|(n, s)| {
                        let trimmed = s.trim_start_matches([' ', '\t']);
                        (!trimmed.is_empty()).then_some((n, trimmed))
                    }) else { return String::new() };
                let Some((end, last)) = s.iter().enumerate().rev()
                    .find_map(|(n, s)| {
                        let trimmed = s.trim_end_matches([' ', '\t']);
                        (!trimmed.is_empty()).then_some((n, trimmed))
                    }) else { return String::new() };

                if start == end {
                    first.trim_end_matches([' ', '\t']).to_owned()
                } else {
                    iter::once(first)
                        .chain(
                            s.into_iter().skip(start + 1)
                                .take(end.saturating_sub(start + 1))
                        )
                        .chain(iter::once(last))
                        .collect::<String>()
                }
            })
            .collect()
    }

pub(crate) rule table0() -> Vec<(usize, Vec<String>)>
    = _ d:((p:position!() r:table_row() { (p, r) }) ++ _) { d }

pub(crate) rule table() -> Table
    = pa:position!() t:table0() pb:position!() {?
        let (offsets, t): (Vec<_>, Vec<_>) = t.into_iter().unzip();
        if let Some(n) = t.iter().position(|x| x.len() != t[0].len()) {
            env.set_fatal_error(EnvError::InconsistentCellCount(t), Some(offsets[n]));
            Err("inconsistent table row sizes")
        } else {
            Ok(Table::builder()
                .span(Span { start: pa, end: pb })
                .position(env.position(pa))
                .rows(t)
                .build())
        }
    }

rule step_argument() -> (Option<Docstring>, Option<Table>)
    = d:docstring() { (Some(d), None) }
    / t:table() { (None, Some(t)) }

pub(crate) rule step() -> Step
    = comment()* pa:position!() k:keyword((env.keywords().given)) _ n:not_nl() pb:position!() _ nl_eof() _
      a:step_argument()?
    {
        let (d, t) = a.unwrap_or_default();
        env.set_last_step(StepType::Given);
        Step::builder().ty(StepType::Given)
            .keyword(k.to_string())
            .value(n.trim_end().to_string())
            .table(t)
            .docstring(d)
            .span(Span { start: pa, end: pb })
            .position(env.position(pa))
            .build()
    }
    / pa:position!() k:keyword((env.keywords().when)) _ n:not_nl() pb:position!() _ nl_eof() _
      a:step_argument()?
    {
        let (d, t) = a.unwrap_or_default();
        env.set_last_step(StepType::When);
        Step::builder().ty(StepType::When)
            .keyword(k.to_string())
            .value(n.trim_end().to_string())
            .table(t)
            .docstring(d)
            .span(Span { start: pa, end: pb })
            .position(env.position(pa))
            .build()
    }
    / pa:position!() k:keyword((env.keywords().then)) _ n:not_nl() pb:position!() _ nl_eof() _
      a:step_argument()?
    {
        let (d, t) = a.unwrap_or_default();
        env.set_last_step(StepType::Then);
        Step::builder().ty(StepType::Then)
            .keyword(k.to_string())
            .value(n.trim_end().to_string())
            .table(t)
            .docstring(d)
            .span(Span { start: pa, end: pb })
            .position(env.position(pa))
            .build()
    }
    / pa:position!() k:keyword((env.keywords().and)) _ n:not_nl() pb:position!() _ nl_eof() _
      a:step_argument()?
    {
        let (d, t) = a.unwrap_or_default();
        let ty = env.last_step().unwrap_or(StepType::Given);
        env.set_last_step(ty);
        Step::builder().ty(ty)
            .keyword(k.to_string())
            .value(n.trim_end().to_string())
            .table(t)
            .docstring(d)
            .span(Span { start: pa, end: pb })
            .position(env.position(pa))
            .build()
    }
    / pa:position!() k:keyword((env.keywords().but)) _ n:not_nl() pb:position!() _ nl_eof() _
      a:step_argument()?
    {
        let (d, t) = a.unwrap_or_default();
        let ty = env.last_step().unwrap_or(StepType::Given);
        env.set_last_step(ty);
        Step::builder().ty(ty)
            .keyword(k.to_string())
            .value(n.trim_end().to_string())
            .table(t)
            .docstring(d)
            .span(Span { start: pa, end: pb })
            .position(env.position(pa))
            .build()
    }

pub(crate) rule steps() -> Vec<Step>
    = s:(step() ** _) {
        env.clear_last_step();
        s
    }

rule background() -> Background
    = comment()* _ pa:position!()
      k:keyword((env.keywords().background)) ":" _ n:not_nl()? nl_eof()
      d:description((&env.keywords().excluded_background()), (&env.keywords().steps()))?
      s:steps()?
      pb:position!()
    {
        Background::builder()
            .keyword(k.into())
            .name(n.unwrap_or_default().trim_end().to_string())
            .description(d.flatten())
            .steps(s.unwrap_or_default())
            .span(Span { start: pa, end: pb })
            .position(env.position(pa))
            .build()
    }

rule any_directive() -> &'static str
    = k:keyword((&*env.keywords().all())) {
        k
    }

// Line ending a description: one starting with any of the `keywords` followed
// by `:`, or with any of the `prefixes`.
rule description_end(keywords: &[&str], prefixes: &[&str])
    = comment()* l:$(not_nl()) {?
        if keywords.iter().any(|k| l.strip_prefix(k).is_some_and(|r| r.starts_with(':')))
            || prefixes.iter().any(|p| l.starts_with(p))
        {
            Ok(())
        } else {
            Err("description line")
        }
    }

rule description_line(keywords: &[&str], prefixes: &[&str]) -> &'input str
    = _
      !"@" !description_end(keywords, prefixes)
      _ n:not_nl() nl_eof()
    {
        n
    }

rule description(keywords: &[&str], prefixes: &[&str]) -> Option<String>
    = d:(description_line(keywords, prefixes) ** _) __ {
        let d = d.join("\n");
        if d.trim() == "" {
            None
        } else {
            Some(d)
        }
    }

rule examples() -> Examples
    = comment()*
      _
      t:tags()
      _
      pa:position!()
      k:keyword((env.keywords().examples)) ":" _ n:not_nl()? nl_eof()
      d:description((&env.keywords().excluded_scenario()), (&["|"]))?
      tb:table()?
      pb:position!()
    {
        Examples::builder()
            .keyword(k.into())
            .name(n.map(|n| n.trim_end().to_string()))
            .description(d.flatten())
            .tags(t)
            .table(tb)
            .span(Span { start: pa, end: pb })
            .position(env.position(pa))
            .build()
    }

rule scenario() -> Scenario
    = comment()*
      _
      t:tags()
      _
      pa:position!()
      k:keyword((env.keywords().scenario)) ":" _ n:not_nl()? _ nl_eof()
      d:description((&env.keywords().excluded_scenario()), (&env.keywords().steps()))?
      s:steps()?
      e:examples()*
      pb:position!()
    {
        Scenario::builder()
            .keyword(k.into())
            .name(n.unwrap_or_default().trim_end().to_string())
            .description(d.flatten())
            .tags(t)
            .steps(s.unwrap_or_default())
            .examples(e)
            .span(Span { start: pa, end: pb })
            .position(env.position(pa))
            .build()
    }
    / comment()*
      _
      t:tags()
      _
      pa:position!()
      k:keyword((env.keywords().scenario_outline)) ":" _ n:not_nl()? _ nl_eof()
      d:description((&env.keywords().excluded_scenario()), (&env.keywords().steps()))?
      s:steps()?
      e:examples()*
      pb:position!()
    {
        Scenario::builder()
            .keyword(k.into())
            .name(n.unwrap_or_default().trim_end().to_string())
            .description(d.flatten())
            .tags(t)
            .steps(s.unwrap_or_default())
            .examples(e)
            .span(Span { start: pa, end: pb })
            .position(env.position(pa))
            .build()
    }

rule tag_char() -> &'input str
    = s:$([_]) {?
        let x = s.chars().next().unwrap();
        if !x.is_whitespace() && x != '@' {
            Ok(s)
        } else {
            Err("tag character")
        }
    }

pub(crate) rule tag() -> String
    = "@" s:tag_char()+ { s.join("") }

rule tag_in_expr_char() -> Option<&'input str>
    = s:$([_]) {?
        let x = s.chars().next().unwrap();
        if !env.escaped() && x == '\\' {
            env.set_escaped(true);
            Ok(None)
        } else if env.escaped() {
            env.set_escaped(false);
            if "\\() ".contains(x) {
                Ok(Some(s))
            } else {
                Err("escaped non-reserved char")
            }
        } else if !x.is_whitespace() && !"@()\\".contains(x) {
            Ok(Some(s))
        } else {
            Err("tag character")
        }
    }

pub(crate) rule tag_in_expr() -> String
    = "@" s:tag_in_expr_char()+ {?
        if env.escaped() {
            env.set_escaped(false);
            Err("escaped end of line")
        } else {
            Ok(s.into_iter().flatten().collect())
        }
    }

pub(crate) rule tags() -> Vec<String>
    = t:(tag() ** __) _ nl()* { t }
    / { vec![] }

rule rule_() -> Rule
    = _
      t:tags()
      _
      pa:position!()
      k:keyword((env.keywords().rule)) ":" _ n:not_nl()? _ nl_eof()
      d:description((&env.keywords().excluded_rule()), (&[]))?
      b:background()? nl()*
      s:scenarios()? nl()*
    //   e:examples()?
      pb:position!()
    {
        Rule::builder()
            .keyword(k.into())
            .name(n.unwrap_or_default().trim_end().to_string())
            .description(d.flatten())
            .tags(t)
            .background(b)
            .scenarios(s.unwrap_or_default())
            .span(Span { start: pa, end: pb })
            .position(env.position(pa))
            .build()
    }

rule rules() -> Vec<Rule>
    = _ r:(rule_() ** _)? { r.unwrap_or_default() }

pub(crate) rule scenarios() -> Vec<Scenario>
    = _ s:(scenario() ** _)? { s.unwrap_or_default() }

pub(crate) rule feature() -> Feature
    = _ language_directive()?
      nl()*
      t:tags()
      pa:position!()
      k:keyword((env.keywords().feature)) ":" _ n:not_nl()? _ nl_eof()
      d:description((&env.keywords().excluded_feature()), (&[]))?
      b:background()? nl()*
      s:scenarios() nl()*
      r:rules() pb:position!()
      nl()*
    {?
        if let Err(e) = env.assert_no_error() {
            Err(e)
        } else {
            Ok(Feature::builder()
                .keyword(k.into())
                .tags(t)
                .name(n.unwrap_or_default().trim_end().to_string())
                .description(d.flatten())
                .background(b)
                .scenarios(s)
                .rules(r)
                .span(Span { start: pa, end: pb })
                .position(env.position(pa))
                .build())
        }
    }

// A document holding only blank lines and comments has no feature. A
// `# language: <code>` line is a directive rather than a comment, so a feature
// has to follow it. Like the reference parser, anything else after the colon
// makes the line a plain comment.
rule empty_line()
    = !(_ "#" _ "language" _ ":" _ ['a'..='z' | 'A'..='Z' | '-' | '_']+ _ (nl0() / eof()))
      _ comment_no_nl()?
rule empty_document() = quiet!{empty_line() ** nl0() eof()}

pub(crate) rule document() -> Option<Feature>
    = empty_document() { None }
    / f:feature() { Some(f) }

pub(crate) rule tag_operation() -> TagOperation = precedence!{
    x:(@) _ "or" _ y:@ { TagOperation::Or(Box::new(x), Box::new(y)) }
    --
    x:(@) _ "and" _ y:@ { TagOperation::And(Box::new(x), Box::new(y)) }
    --
    "not" _ x:(@) { TagOperation::Not(Box::new(x)) }
    --
    "(" _ t:tag_operation() _ ")" _ { t }
    t:tag_in_expr() { TagOperation::Tag(t) }
}

}}

#[cfg(test)]
mod test {
    use std::{collections::HashMap, fs};

    use super::*;

    const FOO: &str = "# language: formal\r\n
@hot-stuff
Section: 4.2. The thing we care about
A description just jammed in here for no reason
@lol @a @rule     @with-spaces
Rule: All gubbins must be placed in the airlock

@bad_idea
Evidence: A gubbins in an airlock
    Given a gubbins
    \"\"\"
    That's a gubbins
    and that is
    and so is that
    \"\"\"
    When a gubbins is forced into this weird corner
    | a | b | c |
    | 1 | 2 | 3 |
    | 4 | 5 | 6 |
    Then a gubbins is proven to be in an airlock
";

    // From Gherkin 6 documentation
    const RULE_WITH_BACKGROUND: &str = "
Feature: Overdue tasks
  Let users know when tasks are overdue, even when using other
  features of the app

  Rule: Users are notified about overdue tasks on first use of the day
    Background:
      Given I have overdue tasks

    Example: First use of the day
      Given I last used the app yesterday
      When I use the app
      Then I am notified about overdue tasks

    Example: Already used today
      Given I last used the app earlier today
      When I use the app
      Then I am not notified about overdue tasks
";

    const DOCSTRING: &str = r#"
Feature: Meow

Scenario: Meow
  Given meow
    """
    Docstring life!
    """
"#;

    const DOCSTRING2: &str = r#"
Feature: Meow

Scenario: Meow
  Given meow
    ```
    Docstring life!
    ```
"#;

    #[test]
    fn smoke() {
        let env = GherkinEnv::default();
        assert!(gherkin_parser::feature(FOO, &env).is_ok());
    }

    #[test]
    fn smoke2() {
        let env = GherkinEnv::default();
        let d = env!("CARGO_MANIFEST_DIR");
        let s = fs::read_to_string(format!("{}/tests/test.feature", d)).unwrap();
        assert!(gherkin_parser::feature(&s, &env).is_ok());
    }

    #[test]
    fn rule_with_background() {
        let env = GherkinEnv::default();
        assert!(
            gherkin_parser::feature(RULE_WITH_BACKGROUND, &env).is_ok(),
            "RULE_IN_BACKGROUND was not parsed correctly!"
        );
    }

    #[test]
    fn docstring() {
        let env = GherkinEnv::default();
        assert!(
            gherkin_parser::feature(DOCSTRING, &env).is_ok(),
            "DOCSTRING was not parsed correctly!"
        );
    }

    #[test]
    fn docstring2() {
        let env = GherkinEnv::default();
        assert!(
            gherkin_parser::feature(DOCSTRING2, &env).is_ok(),
            "DOCSTRING2 was not parsed correctly!"
        );
    }

    #[test]
    fn docstring_media_type_and_indentation() {
        let env = GherkinEnv::default();
        let input = r#"Feature: f
  Scenario: s
    Given a body
      ```json
      {
        "a": 1
      }
      ```
"#;
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let start = input.find("```json").unwrap();
        let end = input.rfind("```").unwrap() + 3;
        assert_eq!(
            feature.scenarios[0].steps[0].docstring,
            Some(
                Docstring::builder()
                    .content("{\n  \"a\": 1\n}".to_owned())
                    .media_type(Some("json".to_owned()))
                    .delimiter("```".to_owned())
                    .span(Span { start, end })
                    .position(LineCol { line: 4, col: 7 })
                    .build()
            ),
        );
    }

    #[test]
    fn docstring_keeps_indentation_past_delimiter() {
        let env = GherkinEnv::default();
        let input = r#"Feature: f
  Scenario: s
    Given a body
      """
        indented
      """
"#;
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let docstring = feature.scenarios[0].steps[0].docstring().unwrap();
        assert_eq!(docstring.content, "  indented");
        assert_eq!(docstring.media_type, None);
        assert_eq!(docstring.delimiter, "\"\"\"");
        assert_eq!(docstring.position, LineCol { line: 4, col: 7 });
    }

    #[test]
    fn docstring_has_no_surrounding_newlines() {
        let env = GherkinEnv::default();
        let input = r#"Feature: f
  Scenario: s
    Given a body
      """
      x
        y
      """
"#;
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let docstring = feature.scenarios[0].steps[0].docstring().unwrap();
        assert_eq!(docstring.content, "x\n  y");
    }

    #[test]
    fn docstring_with_crlf_and_less_indented_line() {
        let env = GherkinEnv::default();
        let input = "Feature: f\r\n  Scenario: s\r\n    Given a body\r\n      ```xml\r\n      <a>\r\n        <b/>\r\n\r\n    </a>\r\n      ```\r\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let docstring = feature.scenarios[0].steps[0].docstring().unwrap();
        assert_eq!(docstring.content, "<a>\n  <b/>\n\n</a>");
        assert_eq!(docstring.media_type.as_deref(), Some("xml"));
        assert_eq!(docstring.position, LineCol { line: 4, col: 7 });
    }

    #[test]
    fn docstring_trims_media_type() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario: s\n    Given a body\n      \"\"\" json \r\n      x\n      \"\"\"\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let docstring = feature.scenarios[0].steps[0].docstring().unwrap();
        assert_eq!(docstring.media_type.as_deref(), Some("json"));
        assert_eq!(docstring.content, "x");
    }

    #[test]
    fn docstring_span_with_double_quotes() {
        let env = GherkinEnv::default();
        let input =
            "Feature: f\n  Scenario: s\n    Given a body\n      \"\"\"\n      x\n      \"\"\"\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let start = input.find("\"\"\"").unwrap();
        let end = input.rfind("\"\"\"").unwrap() + 3;
        let docstring = feature.scenarios[0].steps[0].docstring().unwrap();
        assert_eq!(docstring.span, Span { start, end });
    }

    #[test]
    fn docstring_with_non_ascii_leading_whitespace() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario: s\n    Given a body\n      \"\"\"\n  \u{a0} x\n      \u{a0}y\n      \"\"\"\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let docstring = feature.scenarios[0].steps[0].docstring().unwrap();
        assert_eq!(docstring.content, "x\n\u{a0}y");
    }

    #[test]
    fn docstring_empty() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario: s\n    Given a\n      \"\"\"\n      \"\"\"\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let docstring = feature.scenarios[0].steps[0].docstring().unwrap();
        assert_eq!(docstring.content, "");
        assert_eq!(docstring.media_type, None);
    }

    #[test]
    fn docstring_with_tab_indentation() {
        let env = GherkinEnv::default();
        let input =
            "Feature: f\n  Scenario: s\n    Given a\n\t\t\"\"\"\n\t\tx\n\t\t\ty\n\t\t\"\"\"\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let docstring = feature.scenarios[0].steps[0].docstring().unwrap();
        assert_eq!(docstring.content, "x\n\ty");
        assert_eq!(docstring.position, LineCol { line: 4, col: 3 });
    }

    #[test]
    fn docstring_in_background_and_rule_background() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Background:\n    Given a\n      \"\"\"\n      x\n      \"\"\"\n  Rule: r\n    Background:\n      Given b\n        ```md\n        y\n        ```\n    Scenario: s\n      Then c\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let docstring = feature.background.unwrap().steps[0]
            .docstring()
            .cloned()
            .unwrap();
        assert_eq!(docstring.content, "x");
        assert_eq!(docstring.position, LineCol { line: 4, col: 7 });
        let docstring = feature.rules[0].background.as_ref().unwrap().steps[0]
            .docstring()
            .unwrap();
        assert_eq!(docstring.content, "y");
        assert_eq!(docstring.media_type.as_deref(), Some("md"));
        assert_eq!(docstring.delimiter, "```");
        assert_eq!(docstring.position, LineCol { line: 10, col: 9 });
    }

    #[test]
    fn docstring_containing_the_other_delimiter() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario: s\n    Given a\n      ```\n      \"\"\"\n      y\n      \"\"\"\n      ```\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let docstring = feature.scenarios[0].steps[0].docstring().unwrap();
        assert_eq!(docstring.content, "\"\"\"\ny\n\"\"\"");
        assert_eq!(docstring.delimiter, "```");
    }

    #[test]
    fn docstring_with_unescaped_delimiters() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario: s\n    Given a\n      \"\"\"\n      \\\"\\\"\\\" x\n      \"\"\"\n    Then b\n      ```\n      \\`\\`\\` y\n      ```\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let steps = &feature.scenarios[0].steps;
        assert_eq!(steps[0].docstring().unwrap().content, "\"\"\" x");
        assert_eq!(steps[1].docstring().unwrap().content, "``` y");
    }

    #[test]
    fn docstring_on_one_line_is_rejected() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario: s\n    Given a\n      \"\"\"x\"\"\"\n    Then b\n";
        assert!(gherkin_parser::feature(input, &env).is_err());
    }

    #[test]
    fn feature_name_and_scenario() {
        let env = GherkinEnv::default();
        let input = r#"Feature: Basic functionality
        here's some text
        really
Scenario: Hello
  Given a step"#;
        let feature = gherkin_parser::feature(input, &env).unwrap();
        println!("{:#?}", feature);
        assert_eq!(feature.scenarios.len(), 1);
        assert!(feature.description.is_some());
        assert!(feature.scenarios[0].steps[0].position.line != 0);
    }

    #[test]
    fn correct_line_numbers() {
        let env = GherkinEnv::default();
        let input = r#"
# language: en
Feature: Basic functionality
        here's some text
     really
@tag
Scenario: Hello
  Given a step
  Then a step
@tag
Scenario: Hello
  Given a step

  And more

# comment
Rule: rule
    @tag
    Scenario Outline: Hello
        Given <step>
        """
        Doc String
        """

    Examples:
        | step |
        | 1    |
        | 2    |


    @tag
Rule: rule
    #comment
    Scenario: Hello
        Given a step
"#;
        let feature = gherkin_parser::feature(input, &env).unwrap();
        assert_eq!(feature.scenarios.len(), 2);
        assert!(feature.description.is_some());
        assert_eq!(feature.position.line, 3);
        assert_eq!(feature.scenarios[0].position.line, 7);
        assert_eq!(feature.scenarios[0].steps[0].position.line, 8);
        assert_eq!(feature.scenarios[0].steps[1].position.line, 9);
        assert_eq!(feature.scenarios[1].position.line, 11);
        assert_eq!(feature.scenarios[1].steps[0].position.line, 12);
        assert_eq!(feature.scenarios[1].steps[1].position.line, 14);
        assert_eq!(feature.rules[0].position.line, 17);
        assert_eq!(feature.rules[0].position.line, 17);
        assert_eq!(feature.rules[0].scenarios[0].position.line, 19);
        assert_eq!(feature.rules[0].scenarios[0].steps[0].position.line, 20);
        assert_eq!(feature.rules[0].scenarios[0].examples[0].position.line, 25);
        assert_eq!(
            feature.rules[0].scenarios[0].examples[0]
                .table
                .as_ref()
                .unwrap()
                .position
                .line,
            26,
        );
        assert_eq!(
            feature.rules[0].scenarios[0].examples[0]
                .table
                .as_ref()
                .unwrap()
                .rows
                .len(),
            3,
        );
        assert_eq!(feature.rules[1].position.line, 32);
        assert_eq!(feature.rules[1].scenarios[0].position.line, 34);
        assert_eq!(feature.rules[1].scenarios[0].steps[0].position.line, 35);
    }

    #[test]
    fn and_as_first_step() {
        let env = GherkinEnv::default();
        let input = r#"
Feature: First step with And
  Scenario: And as first step
    And a step
"#;
        let feature = gherkin_parser::feature(input, &env).unwrap();
        assert_eq!(feature.scenarios.len(), 1);
        assert_eq!(feature.scenarios[0].steps.len(), 1);
        assert_eq!(feature.scenarios[0].steps[0].ty, StepType::Given);
    }

    #[test]
    fn but_as_first_step() {
        let env = GherkinEnv::default();
        let input = r#"
Feature: First step with But
  Scenario: But as first step
    But a step
"#;
        let feature = gherkin_parser::feature(input, &env).unwrap();
        assert_eq!(feature.scenarios.len(), 1);
        assert_eq!(feature.scenarios[0].steps.len(), 1);
        assert_eq!(feature.scenarios[0].steps[0].ty, StepType::Given);
    }

    #[test]
    fn scenario_description() {
        let env = GherkinEnv::default();
        let input = r#"
Feature: First step with Besides
  Scenario: Besides as first step
    Besides a step
"#;
        let feature = gherkin_parser::feature(input, &env).unwrap();
        assert_eq!(feature.scenarios.len(), 1);
        assert_eq!(feature.scenarios[0].steps.len(), 0);
    }

    #[test]
    fn feature_only() {
        let env = GherkinEnv::default();
        let input = r#"Feature: Basic functionality
        "#;
        let feature = gherkin_parser::feature(input, &env).unwrap();
        println!("{:#?}", feature);
        assert_eq!(feature.scenarios.len(), 0);
        assert!(feature.description.is_none());
    }

    #[test]
    fn non_child_lines_under_feature_are_description() {
        for (input, description) in [
            ("Feature: f\n  Examples:\n", "Examples:"),
            ("Feature: f\n  Examples: x\n", "Examples: x"),
            ("Feature: f\n  Scenarios:\n", "Scenarios:"),
            ("Feature: f\n  Examples:\n    | a |\n", "Examples:\n| a |"),
            (
                "Feature: f\n  some text\n  Examples:\n",
                "some text\nExamples:",
            ),
            ("Feature: f\n  Scenario Outline x\n", "Scenario Outline x"),
            ("Feature: f\nFeature: g\n", "Feature: g"),
            ("Feature: f\n  Given a start\n", "Given a start"),
            (
                "# language: fr\nFonctionnalité: f\n  Exemples:\n",
                "Exemples:",
            ),
        ] {
            let env = GherkinEnv::default();
            let feature = gherkin_parser::feature(input, &env)
                .unwrap_or_else(|e| panic!("{:?} was not parsed: {}", input, e));
            assert_eq!(feature.description.as_deref(), Some(description));
            assert!(feature.background.is_none());
            assert_eq!(feature.scenarios.len(), 0);
            assert_eq!(feature.rules.len(), 0);
        }
    }

    #[test]
    fn child_lines_under_feature_are_children() {
        for input in [
            "Feature: f\n  Background:\n",
            "Feature: f\n  Scenario: s\n",
            "Feature: f\n  Example: s\n",
            "Feature: f\n  Scenario Outline: s\n",
            "Feature: f\n  Rule: r\n",
            "# language: fr\nFonctionnalité: f\n  Exemple: s\n",
        ] {
            let env = GherkinEnv::default();
            let feature = gherkin_parser::feature(input, &env)
                .unwrap_or_else(|e| panic!("{:?} was not parsed: {}", input, e));
            assert!(feature.description.is_none());
            assert_eq!(
                usize::from(feature.background.is_some())
                    + feature.scenarios.len()
                    + feature.rules.len(),
                1,
                "{:?}",
                input,
            );
        }
    }

    #[test]
    fn examples_under_rule_and_background_are_description() {
        let env = GherkinEnv::default();
        let feature =
            gherkin_parser::feature("Feature: f\n  Rule: r\n    Examples:\n", &env).unwrap();
        assert_eq!(feature.rules[0].description.as_deref(), Some("Examples:"));

        let env = GherkinEnv::default();
        let feature =
            gherkin_parser::feature("Feature: f\n  Background:\n    Examples:\n", &env).unwrap();
        let background = feature.background.unwrap();
        assert_eq!(background.description.as_deref(), Some("Examples:"));
    }

    #[test]
    fn examples_after_scenario_description() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario: s\n    some text\n  Examples:\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let scenario = &feature.scenarios[0];
        assert_eq!(scenario.description.as_deref(), Some("some text"));
        assert_eq!(scenario.examples.len(), 1);
    }

    #[test]
    fn examples_after_scenario_in_other_languages() {
        for input in [
            "# language: it\nFunzionalità: f\n  Scenario: s\n    Esempi:\n      | a |\n",
            "# language: jv\nFitur: f\n  Skenario: s\n    Contone:\n      | a |\n",
        ] {
            let env = GherkinEnv::default();
            let feature = gherkin_parser::feature(input, &env)
                .unwrap_or_else(|e| panic!("{:?} was not parsed: {}", input, e));
            let scenario = &feature.scenarios[0];
            assert!(scenario.description.is_none(), "{:?}", input);
            assert_eq!(scenario.examples.len(), 1, "{:?}", input);
        }
    }

    #[test]
    fn longest_keyword_wins() {
        let env = GherkinEnv::default();
        let input = "# language: es\nCaracterística: f\n  Regla de negocio: r\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        assert_eq!(feature.rules[0].keyword, "Regla de negocio");
        assert_eq!(feature.rules[0].name, "r");
    }

    #[test]
    fn longest_step_keyword_wins() {
        let env = GherkinEnv::default();
        let input = "# language: sk\nPožiadavka: f\n  Scenár: s\n    A tiež y\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let step = &feature.scenarios[0].steps[0];
        assert_eq!(step.keyword, "A tiež ");
        assert_eq!(step.value, "y");
    }

    #[test]
    fn examples_description_may_start_with_step_keyword() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario Outline: s\n    Given <a>\n    Examples:\n      Given text\n      | a |\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let examples = &feature.scenarios[0].examples[0];
        assert_eq!(examples.description.as_deref(), Some("Given text"));
        assert_eq!(examples.table.as_ref().unwrap().rows.len(), 1);
    }

    #[test]
    fn examples_after_scenario_outline_description() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario Outline: s\n    some text\n    Given <a>\n    Examples:\n      | a |\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let outline = &feature.scenarios[0];
        assert_eq!(outline.description.as_deref(), Some("some text"));
        assert_eq!(outline.examples.len(), 1);

        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario Outline: s\n    some text\n  Examples:\n      | a |\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let outline = &feature.scenarios[0];
        assert_eq!(outline.description.as_deref(), Some("some text"));
        assert_eq!(outline.examples.len(), 1);
    }

    #[test]
    fn description_keeps_step_keyword_in_the_middle() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario: s\n    I have a Given thing\n    Given x\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let scenario = &feature.scenarios[0];
        assert_eq!(
            scenario.description.as_deref(),
            Some("I have a Given thing")
        );
        assert_eq!(scenario.steps.len(), 1);
    }

    #[test]
    fn consecutive_examples_are_separate_blocks() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario Outline: s\n    Given <a>\n    Examples: one\n    Examples: two\n      | a |\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        assert_eq!(feature.scenarios[0].examples.len(), 2);
    }

    #[test]
    fn scenario_after_examples_without_table() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario Outline: s\n    Given <a>\n    Examples:\n  Scenario: t\n    Given x\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        assert_eq!(feature.scenarios.len(), 2);
    }

    #[test]
    fn scenario_outline_ends_background_description() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Background:\n    text\n  Scenario Outline: o\n    Given <a>\n    Examples:\n      | a |\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let background = feature.background.unwrap();
        assert_eq!(background.description.as_deref(), Some("text"));
        assert_eq!(feature.scenarios.len(), 1);
    }

    #[test]
    fn every_step_keyword_ends_scenario_description() {
        for (kw, ty) in [
            ("Given", StepType::Given),
            ("When", StepType::When),
            ("Then", StepType::Then),
            ("And", StepType::Given),
            ("But", StepType::Given),
        ] {
            let env = GherkinEnv::default();
            let input = format!("Feature: f\n  Scenario: s\n    text\n    {kw} x\n");
            let feature = gherkin_parser::feature(&input, &env).unwrap();
            let scenario = &feature.scenarios[0];
            assert_eq!(scenario.description.as_deref(), Some("text"), "{kw}");
            assert_eq!(scenario.steps.len(), 1, "{kw}");
            assert_eq!(scenario.steps[0].ty, ty, "{kw}");
        }
    }

    #[test]
    fn step_keyword_text_under_rule_is_description() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Rule: r\n    Given text\n  Scenario: s\n    Given x\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        assert_eq!(feature.rules[0].description.as_deref(), Some("Given text"));
    }

    #[test]
    fn comments_and_tags_around_feature_description() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  text\n  # comment\n  Examples:\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        assert_eq!(feature.description.as_deref(), Some("text\nExamples:"));

        let env = GherkinEnv::default();
        let input = "Feature: f\n  Examples\n  @tag\n  Scenario: s\n    Given x\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        assert_eq!(feature.description.as_deref(), Some("Examples"));
        assert_eq!(feature.scenarios.len(), 1);
    }

    #[test]
    fn escape_examples() {
        let env = GherkinEnv::default();
        let input = r#"
Feature: Foo
  Scenario: Bar
    Examples:
      | value |
      |  alu  |
      |   l   |
      |       |
      | \n    |
      | \|    |
      | \\    |
      | \\n   |
      | \\\\  |
"#;
        let feature = gherkin_parser::feature(input, &env).unwrap();

        assert_eq!(
            feature.scenarios[0].examples[0]
                .table
                .as_ref()
                .unwrap()
                .rows,
            vec![
                vec!["value".to_owned()],
                vec!["alu".to_owned()],
                vec!["l".to_owned()],
                vec!["".to_owned()],
                vec!["\n".to_owned()],
                vec!["|".to_owned()],
                vec!["\\".to_owned()],
                vec!["\\n".to_owned()],
                vec!["\\\\".to_owned()],
            ]
        );
    }

    #[test]
    fn keep_unknown_example_escape_sequence() {
        let env = GherkinEnv::default();
        let input = r#"
Feature: Foo
  Scenario: Bar
    Examples:
      | value |
      | \     |
      | g\h   |
      | g\    |
      | \a\b  |
      | x\\y  |
      | \\\   |
      | \x\| |
"#;
        let feature = gherkin_parser::feature(input, &env).unwrap();

        assert_eq!(
            feature.scenarios[0].examples[0]
                .table
                .as_ref()
                .unwrap()
                .rows,
            vec![
                vec!["value".to_owned()],
                vec!["\\".to_owned()],
                vec!["g\\h".to_owned()],
                vec!["g\\".to_owned()],
                vec!["\\a\\b".to_owned()],
                vec!["x\\y".to_owned()],
                vec!["\\\\".to_owned()],
                vec!["\\x|".to_owned()],
            ]
        );
    }

    #[test]
    fn keep_unknown_step_table_escape_sequence() {
        let env = GherkinEnv::default();
        let input = r#"
Feature: f
  Scenario: s
    Given a table
      | g\h |
"#;
        let feature = gherkin_parser::feature(input, &env).unwrap();

        assert_eq!(
            feature.scenarios[0].steps[0].table.as_ref().unwrap().rows,
            vec![vec!["g\\h".to_owned()]],
        );
    }

    #[test]
    fn reject_trailing_backslash_before_line_break_in_table_cell() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario: s\n    Given t\n      | a\\\n      | b |\n";
        assert!(gherkin_parser::feature(input, &env).is_err());
    }

    #[test]
    fn keep_backslash_before_closing_pipe_in_table_cell() {
        let env = GherkinEnv::default();
        let input = "Feature: f\n  Scenario: s\n    Given t\n      |a\\ |\n      | \\\t|\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        assert_eq!(
            feature.scenarios[0].steps[0].table.as_ref().unwrap().rows,
            vec![vec!["a\\".to_owned()], vec!["\\".to_owned()]],
        );
    }

    #[test]
    fn reject_step_table_row_longer_than_first() {
        let env = GherkinEnv::default();
        let input = r#"
Feature: Foo
  Scenario: Bar
    Given a table
      | a | b |
      | 1 | 2 | 3 |
"#;
        assert!(gherkin_parser::feature(input, &env).is_err());
        match env.fatal_error.into_inner() {
            Some(EnvError::InconsistentCellCount(rows)) => assert_eq!(
                rows,
                vec![
                    vec!["a".to_owned(), "b".to_owned()],
                    vec!["1".to_owned(), "2".to_owned(), "3".to_owned()],
                ],
            ),
            e => panic!("expected `InconsistentCellCount`, got: {:?}", e),
        }
    }

    #[test]
    fn reject_examples_table_row_longer_than_first() {
        let env = GherkinEnv::default();
        let input = r#"
Feature: Foo
  Scenario Outline: Bar
    Given the <what>

    Examples:
      | what |
      | x    | extra |
"#;
        assert!(gherkin_parser::feature(input, &env).is_err());
        match env.fatal_error.into_inner() {
            Some(EnvError::InconsistentCellCount(rows)) => assert_eq!(
                rows,
                vec![
                    vec!["what".to_owned()],
                    vec!["x".to_owned(), "extra".to_owned()],
                ],
            ),
            e => panic!("expected `InconsistentCellCount`, got: {:?}", e),
        }
    }

    #[test]
    fn reject_step_table_row_shorter_than_first() {
        let env = GherkinEnv::default();
        let input = r#"
Feature: Foo
  Scenario: Bar
    Given a table
      | a | b |
      | 1 |
"#;
        assert!(gherkin_parser::feature(input, &env).is_err());
        match env.fatal_error.into_inner() {
            Some(EnvError::InconsistentCellCount(rows)) => assert_eq!(
                rows,
                vec![vec!["a".to_owned(), "b".to_owned()], vec!["1".to_owned()]],
            ),
            e => panic!("expected `InconsistentCellCount`, got: {:?}", e),
        }
    }

    #[test]
    fn reject_examples_table_row_shorter_than_first() {
        let env = GherkinEnv::default();
        let input = r#"
Feature: Foo
  Scenario Outline: Bar
    Given the <what>

    Examples:
      | what | other |
      | x    |
"#;
        assert!(gherkin_parser::feature(input, &env).is_err());
        match env.fatal_error.into_inner() {
            Some(EnvError::InconsistentCellCount(rows)) => assert_eq!(
                rows,
                vec![
                    vec!["what".to_owned(), "other".to_owned()],
                    vec!["x".to_owned()],
                ],
            ),
            e => panic!("expected `InconsistentCellCount`, got: {:?}", e),
        }
    }

    #[test]
    fn reject_later_table_row_longer_than_first() {
        let env = GherkinEnv::default();
        let input = r#"
Feature: Foo
  Background:
    Given a table
      | a | b |
      | 1 | 2 |
      | 3 | 4 | 5 |
"#;
        assert!(gherkin_parser::feature(input, &env).is_err());
        assert!(matches!(
            env.fatal_error.into_inner(),
            Some(EnvError::InconsistentCellCount(_)),
        ));
    }

    #[test]
    fn reject_table_row_with_trailing_empty_cell() {
        let env = GherkinEnv::default();
        let input = r#"
Feature: Foo
  Scenario: Bar
    Given a table
      | a | b |
      | 1 | 2 | |
"#;
        assert!(gherkin_parser::feature(input, &env).is_err());
        assert!(matches!(
            env.fatal_error.into_inner(),
            Some(EnvError::InconsistentCellCount(_)),
        ));
    }

    #[test]
    fn accept_table_row_with_escaped_pipe() {
        let env = GherkinEnv::default();
        let input = r#"
Feature: Foo
  Scenario: Bar
    Given a table
      | a | b |
      | 1 \| 2 | 3 |
"#;
        let f = gherkin_parser::feature(input, &env).unwrap();
        let table = f.scenarios[0].steps[0].table.as_ref().unwrap();
        assert_eq!(table.rows[1], vec!["1 | 2".to_owned(), "3".to_owned()]);
    }

    #[test]
    fn reject_step_with_docstring_and_table() {
        let env = GherkinEnv::default();
        for keyword in ["Given", "When", "Then", "And", "But", "*"] {
            for fence in ["\"\"\"", "```"] {
                let input = format!(
                    "Feature: Foo\n  Scenario: Bar\n    Given a\n    \
                     {keyword} b\n      {fence}\n      x\n      {fence}\n      \
                     | a |\n",
                );
                assert!(
                    gherkin_parser::feature(&input, &env).is_err(),
                    "accepted:\n{}",
                    input,
                );
            }
        }
    }

    #[test]
    fn reject_step_with_table_and_docstring() {
        let env = GherkinEnv::default();
        for keyword in ["Given", "When", "Then", "And", "But", "*"] {
            for fence in ["\"\"\"", "```"] {
                let input = format!(
                    "Feature: Foo\n  Scenario: Bar\n    Given a\n    \
                     {keyword} b\n      | a |\n      {fence}\n      x\n      \
                     {fence}\n",
                );
                assert!(
                    gherkin_parser::feature(&input, &env).is_err(),
                    "accepted:\n{}",
                    input,
                );
            }
        }
    }

    #[test]
    fn reject_step_with_docstring_and_table_in_other_contexts() {
        let env = GherkinEnv::default();
        let step = "      \"\"\"\n      x\n      \"\"\"\n      | a |\n";
        for head in [
            "Feature: Foo\n  Background:\n    Given a\n",
            "Feature: Foo\n  Scenario Outline: Bar\n    Given a\n",
            "Feature: Foo\n  Rule: R\n    Scenario: Bar\n      Given a\n",
        ] {
            let input = format!("{head}{step}");
            assert!(
                gherkin_parser::feature(&input, &env).is_err(),
                "accepted:\n{}",
                input,
            );
        }
    }

    #[test]
    fn document_without_feature() {
        for input in [
            "",
            "\n",
            "  \n\t\n",
            "# only a comment\n",
            "# only a comment",
            "# a\n\n  # b\r\n",
            "# languages are fun\n",
            "# language is not a directive\n",
            "# language:\n",
            "# language:   \n",
            "# language: not a lang\n",
            "# language: fr extra\n",
            "# language: fr # x\n",
        ] {
            let env = GherkinEnv::default();
            assert_eq!(
                gherkin_parser::document(input, &env).unwrap(),
                None,
                "{:?}",
                input,
            );
        }
    }

    #[test]
    fn reject_step_with_docstring_comment_and_table() {
        let env = GherkinEnv::default();
        let input = "Feature: Foo\n  Scenario: Bar\n    Given a\n      \
                     \"\"\"\n      x\n      \"\"\"\n\n      # note\n      | a |\n";
        assert!(gherkin_parser::feature(input, &env).is_err());
    }

    #[test]
    fn accept_step_with_single_argument() {
        let env = GherkinEnv::default();
        let input = "Feature: Foo\n  Scenario: Bar\n    Given a\n      \
                     \"\"\"\n      x\n      \"\"\"\n    When b\n      | a |\n    \
                     Then c\n";
        let feature = gherkin_parser::feature(input, &env).unwrap();
        let steps = &feature.scenarios[0].steps;
        assert_eq!(steps.len(), 3);
        assert!(steps[0].docstring.is_some() && steps[0].table.is_none());
        assert!(steps[1].docstring.is_none() && steps[1].table.is_some());
        assert!(steps[2].docstring.is_none() && steps[2].table.is_none());
    }

    #[test]
    fn reject_document_without_feature_after_header() {
        for input in [
            "# language: fr\n",
            "# a\n# language: fr\n",
            "# language: xx-invalid\n",
            "  # language: fr\n",
            "\t# language: fr\n",
            "# language : fr\n",
            "# language: fr   \n",
            "@tag\n",
            "Rule: Foo\n",
            "Scenario: Foo\n",
            "Background:\n",
        ] {
            let env = GherkinEnv::default();
            assert!(
                gherkin_parser::document(input, &env).is_err(),
                "{:?}",
                input
            );
        }
    }

    #[test]
    fn fixture_good() {
        let mut failed = 0;

        let d = env!("CARGO_MANIFEST_DIR");
        let files = fs::read_dir(format!("{}/tests/fixtures/data/good/", d)).unwrap();
        for file in files {
            let file = file.unwrap();
            let filename = file.file_name();
            let filename = filename.to_str().unwrap();
            if filename.ends_with(".feature") {
                let res = std::panic::catch_unwind(|| {
                    let env = GherkinEnv::default();
                    let input =
                        fs::read_to_string(format!("{}/tests/fixtures/data/good/{}", d, filename,))
                            .unwrap();
                    let feature = gherkin_parser::document(&input, &env).unwrap();
                    let fixture = fs::read_to_string(format!(
                        "{}/tests/fixtures/data/good/{}.ast.ndjson",
                        d, filename,
                    ))
                    .unwrap();

                    check_ast(&feature, &fixture);
                });
                if res.is_err() {
                    failed += 1;
                    println!("{}", filename);
                }
            }
        }

        assert_eq!(failed, 0, "{} fixtures have failed", failed);
    }

    #[test]
    fn fixture_fail() {
        let d = env!("CARGO_MANIFEST_DIR");
        let files = fs::read_dir(format!("{}/tests/fixtures/data/bad/", d)).unwrap();
        for file in files {
            let file = file.unwrap();
            let filename = file.file_name();
            let filename = filename.to_str().unwrap();
            if filename.ends_with(".feature") {
                let res = std::panic::catch_unwind(|| {
                    let env = GherkinEnv::default();
                    let input =
                        fs::read_to_string(format!("{}/tests/fixtures/data/bad/{}", d, filename,))
                            .unwrap();
                    gherkin_parser::document(&input, &env).unwrap()
                });

                assert!(res.is_err(), "{}: {:?}", filename, res.unwrap());
            }
        }
    }

    /// Parses the `input` expecting it to fail, and returns the position and
    /// the error `Feature::parse_path()` reports for it.
    ///
    /// Also checks the position against the one `Feature::parse()` reports.
    fn parse_error(input: &str) -> (LineCol, Option<EnvError>) {
        // Like `Feature::parse()` and `Feature::parse_path()` do.
        let appended_newline = !input.ends_with('\n');
        let terminated = if appended_newline {
            format!("{input}\n")
        } else {
            input.to_owned()
        };

        let env = GherkinEnv::default();
        let e = gherkin_parser::feature(&terminated, &env).unwrap_err();
        let position = env.error_position(&terminated, appended_newline, e.location.offset);
        let error = env.fatal_error.take().or_else(|| env.last_error.take());

        let parsed = Feature::parse(input, GherkinEnv::default()).unwrap_err();
        assert_eq!(parsed.position, position);

        (position, error)
    }

    #[test]
    fn error_at_inconsistent_table_row() {
        // Reference: `(5:7): inconsistent cell count within the table`
        let input = r#"Feature: f
  Scenario: s
    Given a table
      | a | b |
      | c |
"#;
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 5, col: 7 });
        assert!(
            matches!(&error, Some(EnvError::InconsistentCellCount(_))),
            "{:?}",
            error,
        );
    }

    #[test]
    fn error_at_unknown_line_after_step() {
        // Reference: `(4:5): expected: #EOF, ..., got 'loose words'`
        let input = r#"Feature: f
  Scenario: s
    Given a
    loose words
"#;
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 4, col: 5 });
        assert!(
            matches!(&error, Some(EnvError::UnknownKeyword(s)) if s == "loose words"),
            "{:?}",
            error,
        );
    }

    #[test]
    fn error_at_second_feature() {
        // Reference: `(5:1): expected: #EOF, ..., got 'Feature: g'`
        let input = r#"Feature: f
  Scenario: s
    Given a

Feature: g
  Scenario: t
    Given b
"#;
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 5, col: 1 });
        assert!(
            matches!(&error, Some(EnvError::UnknownKeyword(s)) if s == "Feature: g"),
            "{:?}",
            error,
        );
    }

    #[test]
    fn error_at_end_of_unclosed_docstring() {
        // Reference: `(6:0): unexpected end of file, expected: ...`
        let input = r#"Feature: f
  Scenario: s
    Given a
      """
      x
"#;
        let (position, _) = parse_error(input);
        assert_eq!(position, LineCol { line: 6, col: 0 });
    }

    #[test]
    fn error_at_table_row_longer_than_first() {
        // Reference: `(5:7): inconsistent cell count within the table`
        let input = "Feature: f\n  Scenario: s\n    Given a table\n      | a | b |\n      | c | d | e |\n      | f | g |\n";
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 5, col: 7 });
        assert!(
            matches!(&error, Some(EnvError::InconsistentCellCount(_))),
            "{:?}",
            error,
        );
    }

    #[test]
    fn error_at_inconsistent_table_row_in_the_middle() {
        // Reference: `(5:7): inconsistent cell count within the table`
        let input = "Feature: f\n  Scenario: s\n    Given a table\n      | a | b |\n      | c |\n      | e | f |\n      | g | h |\n";
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 5, col: 7 });
        assert!(
            matches!(&error, Some(EnvError::InconsistentCellCount(_))),
            "{:?}",
            error,
        );
    }

    #[test]
    fn first_fatal_error_wins_over_later_one() {
        // Reference: `(1:1): Language not supported: xx`
        let input = "# language: xx\nFeature: f\n  Scenario: s\n    Given a table\n      | a | b |\n      | c |\n";
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 1, col: 1 });
        assert!(
            matches!(&error, Some(EnvError::UnsupportedLanguage(s)) if s == "xx"),
            "{:?}",
            error,
        );
    }

    #[test]
    fn error_at_tab_indented_line() {
        let input = "Feature: f\n  Scenario: s\n    Given a\n\t\tloose\n";
        let (position, _) = parse_error(input);
        assert_eq!(position, LineCol { line: 4, col: 3 });
    }

    #[test]
    fn error_at_non_ascii_indented_line() {
        let input = "Feature: f\n  Scenario: s\n    Given a\n\u{a0}\u{a0}loose\n";
        let (position, _) = parse_error(input);
        assert_eq!(position, LineCol { line: 4, col: 3 });
    }

    #[test]
    fn error_at_crlf_line() {
        let input = "Feature: f\r\n  Scenario: s\r\n    Given a\r\n    loose words\r\n";
        let (position, _) = parse_error(input);
        assert_eq!(position, LineCol { line: 4, col: 5 });
    }

    #[test]
    fn error_at_end_after_trailing_blank_lines() {
        let input = "Feature: f\n  Scenario: s\n    Given a\n      \"\"\"\n      x\n\n\n";
        let (position, _) = parse_error(input);
        assert_eq!(position, LineCol { line: 8, col: 0 });
    }

    #[test]
    fn error_at_step_after_tag() {
        // Reference: `(5:5): expected: #TagLine, ..., got 'When b'`
        let input = r#"Feature: f
  Scenario: s
    Given a
    @t
    When b
"#;
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 5, col: 5 });
        assert!(
            matches!(&error, Some(EnvError::UnknownKeyword(s)) if s == "When b"),
            "{:?}",
            error,
        );
    }

    #[test]
    fn error_at_unsupported_language_after_blank_lines() {
        // Reference: `(3:3): Language not supported: xx`
        let input = "\n\n  # language: xx\nFeature: f\n  Scenario: s\n    Given a\n";
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 3, col: 3 });
        assert!(
            matches!(&error, Some(EnvError::UnsupportedLanguage(s)) if s == "xx"),
            "{:?}",
            error,
        );
    }

    #[test]
    fn error_at_inconsistent_examples_row() {
        // Reference: `(7:7): inconsistent cell count within the table`
        let input = "Feature: f\n  Scenario Outline: s\n    Given <a>\n    Examples:\n      | a | b |\n      | 1 | 2 |\n      | 3 |\n";
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 7, col: 7 });
        assert!(
            matches!(&error, Some(EnvError::InconsistentCellCount(_))),
            "{:?}",
            error,
        );
    }

    #[test]
    fn error_at_inconsistent_background_row() {
        // Reference: `(5:7): inconsistent cell count within the table`
        let input = "Feature: f\n  Background:\n    Given a table\n      | a | b |\n      | c |\n  Scenario: s\n    Given b\n";
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 5, col: 7 });
        assert!(
            matches!(&error, Some(EnvError::InconsistentCellCount(_))),
            "{:?}",
            error,
        );
    }

    #[test]
    fn error_at_unknown_line_in_rule() {
        // Reference: `(5:7): expected: #EOF, ..., got 'loose words'`
        let input = "Feature: f\n  Rule: r\n    Scenario: s\n      Given a\n      loose words\n";
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 5, col: 7 });
        assert!(
            matches!(&error, Some(EnvError::UnknownKeyword(s)) if s == "loose words"),
            "{:?}",
            error,
        );
    }

    #[test]
    fn error_not_moved_by_comments_and_blank_lines_after_it() {
        // Reference: `(5:7): inconsistent cell count within the table`
        let input = "Feature: f\n  Scenario: s\n    Given a table\n      | a | b |\n      | c |\n\n      # comment\n\n    Given b\n";
        let (position, _) = parse_error(input);
        assert_eq!(position, LineCol { line: 5, col: 7 });

        // Reference: `(4:5): expected: #EOF, ..., got 'loose words'`
        let input = "Feature: f\n  Scenario: s\n    Given a\n    loose words\n    # comment\n\n    Given b\n";
        let (position, _) = parse_error(input);
        assert_eq!(position, LineCol { line: 4, col: 5 });
    }

    #[test]
    fn error_text_has_no_trailing_whitespace() {
        // Reference: `(4:5): expected: #EOF, ..., got 'loose words'`
        let input = "Feature: f\n  Scenario: s\n    Given a\n    loose words \t \n";
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 4, col: 5 });
        assert!(
            matches!(&error, Some(EnvError::UnknownKeyword(s)) if s == "loose words"),
            "{:?}",
            error,
        );
    }

    #[test]
    fn error_at_end_of_input_without_trailing_newline() {
        // Reference: `(6:0): unexpected end of file, expected: ...`
        let input = "Feature: f\n  Scenario: s\n    Given a\n      \"\"\"\n      x";
        let (position, _) = parse_error(input);
        assert_eq!(position, LineCol { line: 6, col: 0 });
    }

    #[test]
    fn error_after_non_ascii_lines() {
        // Reference: `(4:5): expected: #EOF, ..., got 'loose words'`
        let input = "Feature: 日本語\n  Scenario: シナリオ\n    Given あ\n    loose words\n";
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 4, col: 5 });
        assert!(
            matches!(&error, Some(EnvError::UnknownKeyword(s)) if s == "loose words"),
            "{:?}",
            error,
        );
    }

    #[test]
    fn error_at_end_after_whitespace_only_last_line() {
        // Reference: `(6:0): unexpected end of file, expected: ...`, as it
        // drops a last line holding only whitespace.
        for last in ["    ", "\t", "\u{a0}", "\u{feff}"] {
            let input =
                format!("Feature: f\n  Scenario: s\n    Given a\n      \"\"\"\n      x\n{last}",);
            let (position, _) = parse_error(&input);
            assert_eq!(position, LineCol { line: 6, col: 0 }, "{last:?}");
        }

        // Reference: `(2:0): unexpected end of file, expected: ...`
        let (position, _) = parse_error("@t\n   ");
        assert_eq!(position, LineCol { line: 2, col: 0 });
    }

    #[test]
    fn error_at_end_after_whitespace_only_last_line_with_newline() {
        // Reference: `(7:0): unexpected end of file, expected: ...`, as it
        // only drops the empty line after the last newline.
        let input = "Feature: f\n  Scenario: s\n    Given a\n      \"\"\"\n      x\n    \n";
        let (position, _) = parse_error(input);
        assert_eq!(position, LineCol { line: 7, col: 0 });
    }

    #[test]
    fn error_at_end_after_last_line_of_next_line_character() {
        // Reference: `(7:0): unexpected end of file, expected: ...`, as U+0085
        // is not whitespace for it.
        let input = "Feature: f\n  Scenario: s\n    Given a\n      \"\"\"\n      x\n\u{85}";
        let (position, _) = parse_error(input);
        assert_eq!(position, LineCol { line: 7, col: 0 });
    }

    #[test]
    fn error_indented_with_byte_order_mark() {
        // Reference: `(4:2): expected: #EOF, ..., got 'loose'`, as U+FEFF is
        // whitespace for it.
        let input = "Feature: f\n  Scenario: s\n    Given a\n\u{feff}loose\n";
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 4, col: 2 });
        assert!(
            matches!(&error, Some(EnvError::UnknownKeyword(s)) if s == "loose"),
            "{:?}",
            error,
        );

        // Reference: `(1:2): Language not supported: xx`
        let input = "\u{feff}# language: xx\nFeature: f\n";
        let (position, _) = parse_error(input);
        assert_eq!(position, LineCol { line: 1, col: 2 });
    }

    #[test]
    fn error_not_indented_with_next_line_character() {
        // Reference: `(4:1): expected: #EOF, ..., got '\u{85}loose'`, as U+0085
        // is not whitespace for it.
        let input = "Feature: f\n  Scenario: s\n    Given a\n\u{85}loose\n";
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 4, col: 1 });
        assert!(
            matches!(&error, Some(EnvError::UnknownKeyword(s)) if s == "\u{85}loose"),
            "{:?}",
            error,
        );
    }

    #[test]
    fn error_after_line_blank_only_after_trimming() {
        // The grammar does not see these lines as blank, but the reference
        // does, so it moves on to the next line.
        // Reference: `(5:5): expected: #EOF, ..., got 'loose'`
        for blank in ["\u{a0}\u{a0}", "\u{feff}", "\u{b}"] {
            let input = format!("Feature: f\n  Scenario: s\n    Given a\n{blank}\n    loose\n");
            let (position, _) = parse_error(&input);
            assert_eq!(position, LineCol { line: 5, col: 5 }, "{blank:?}");
        }

        // Reference: `(4:1): expected: #EOF, ..., got '\u{85}'`, as U+0085
        // is not whitespace for it.
        let input = "Feature: f\n  Scenario: s\n    Given a\n\u{85}\n    loose\n";
        let (position, _) = parse_error(input);
        assert_eq!(position, LineCol { line: 4, col: 1 });
    }

    #[test]
    fn error_at_first_inconsistent_table_row() {
        // Reference: `(5:7): inconsistent cell count within the table`
        let input = "Feature: f\n  Scenario: s\n    Given a\n      | a | b |\n      | c |\n      | d | e | f |\n";
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 5, col: 7 });
        assert!(
            matches!(&error, Some(EnvError::InconsistentCellCount(_))),
            "{:?}",
            error,
        );
    }

    #[test]
    fn error_text_has_no_surrounding_whitespace() {
        // Reference: `(4:2): expected: #EOF, ..., got 'loose'`
        let input = "Feature: f\n  Scenario: s\n    Given a\n\u{a0}loose\u{3000}\u{a0}\t\n";
        let (position, error) = parse_error(input);
        assert_eq!(position, LineCol { line: 4, col: 2 });
        assert!(
            matches!(&error, Some(EnvError::UnknownKeyword(s)) if s == "loose"),
            "{:?}",
            error,
        );

        // Reference: `(4:1): expected: #EOF, ..., got 'loose\u{85}'`
        let input = "Feature: f\n  Scenario: s\n    Given a\nloose\u{85}\n";
        let (_, error) = parse_error(input);
        assert!(
            matches!(&error, Some(EnvError::UnknownKeyword(s)) if s == "loose\u{85}"),
            "{:?}",
            error,
        );
    }

    // TODO: Actually generate `.ndjson` file from the AST to fully assert
    //       fixtures.
    fn check_ast(parsed: &Option<Feature>, ast_parsed: &str) {
        let d: HashMap<String, serde_json::Value> = serde_json::from_str(ast_parsed).unwrap();

        let document = d
            .get("gherkinDocument")
            .expect("There is no document in the file");
        let Some(feature) = document.get("feature") else {
            assert_eq!(
                parsed, &None,
                "Feature is parsed, but the document has none"
            );
            return;
        };
        let parsed = parsed
            .as_ref()
            .expect("Feature is not parsed, but the document has one");
        let children = feature.get("children");

        if children.is_none() {
            assert!(parsed.background.is_none());
            assert_eq!(parsed.scenarios.len(), 0);
            assert_eq!(parsed.rules.len(), 0);
            return;
        }

        let mut backgrounds = 0;
        let mut scenarios = 0;
        let mut rules = 0;

        let children = children.unwrap().as_array().unwrap();
        for child in children {
            if let Some(background) = child.get("background") {
                let parsed_background = parsed.background.as_ref().unwrap();
                let name = background.get("name").unwrap().as_str().unwrap();

                assert_eq!(&parsed_background.name, name);

                let steps = background.get("steps");

                if steps.is_none() {
                    assert_eq!(parsed_background.steps.len(), 0);
                    continue;
                }

                let steps = steps
                    .expect("steps")
                    .as_array()
                    .expect("Steps must be an array");
                assert!(
                    check_steps(&parsed_background.steps, steps),
                    "Background steps are different from fixture",
                );

                backgrounds += 1;
            } else if let Some(json_scenario) = child.get("scenario") {
                assert!(
                    check_scenario(&parsed.scenarios, json_scenario),
                    "Scenario steps are different from fixture",
                );

                scenarios += 1;
            } else if let Some(json_rule) = child.get("rule") {
                let json_rule_scenarios = json_rule
                    .get("children")
                    .and_then(serde_json::Value::as_array)
                    .map(|children| {
                        children
                            .iter()
                            .filter_map(|child| child.get("scenario"))
                            .collect::<Vec<_>>()
                    });
                let json_rule_name = json_rule.get("name").unwrap().as_str().unwrap();

                let exists = parsed.rules.iter().any(|rule| {
                    if rule.name == json_rule_name {
                        return if let Some(json_scenarios) = &json_rule_scenarios {
                            json_scenarios.len() == rule.scenarios.len()
                                && json_scenarios.iter().all(|json_scenario| {
                                    check_scenario(&rule.scenarios, json_scenario)
                                })
                        } else {
                            rule.scenarios.is_empty()
                        };
                    }
                    false
                });

                assert!(exists, "Scenario rules are different from fixture");

                rules += 1;
            } else {
                panic!("Unknown child type: {:#?}", child);
            }
        }

        if parsed.background.is_some() {
            assert_eq!(1, backgrounds);
        } else {
            assert_eq!(0, backgrounds);
        }

        assert_eq!(parsed.scenarios.len(), scenarios);
        assert_eq!(parsed.rules.len(), rules);
    }

    fn check_scenario(parsed: &[Scenario], json: &serde_json::Value) -> bool {
        let json_steps = json.get("steps").and_then(serde_json::Value::as_array);
        let json_scenario_name = json.get("name").unwrap().as_str().unwrap();

        parsed.iter().any(|parsed| {
            if parsed.name == json_scenario_name {
                return if let Some(json_steps) = json_steps {
                    check_steps(&parsed.steps, json_steps)
                } else {
                    parsed.steps.is_empty()
                };
            }
            false
        })
    }

    fn check_steps(parsed: &[Step], json: &[serde_json::Value]) -> bool {
        if parsed.len() != json.len() {
            return false;
        }

        for (step, json) in parsed.iter().zip(json) {
            if step.keyword != json.get("keyword").unwrap().as_str().unwrap() {
                return false;
            }
            if step.value != json.get("text").unwrap().as_str().unwrap() {
                return false;
            }
            match (&step.docstring, json.get("docString")) {
                (None, None) => {}
                (Some(docstring), Some(json)) => {
                    if json["content"] != docstring.content
                        || json["delimiter"] != docstring.delimiter
                        || json.get("mediaType").and_then(serde_json::Value::as_str)
                            != docstring.media_type.as_deref()
                        || json["location"]["line"] != docstring.position.line
                        || json["location"]["column"] != docstring.position.col
                    {
                        return false;
                    }
                }
                _ => return false,
            }
        }

        true
    }
}
