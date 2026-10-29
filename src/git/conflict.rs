//! Parser de marcadores de conflito (`<<<<<<<`, `|||||||`, `=======`, `>>>>>>>`)
//! para a ferramenta de merge de 3 painéis.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Common(Vec<String>),
    Conflict(Conflict),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub ours: Vec<String>,
    pub base: Vec<String>,
    pub theirs: Vec<String>,
    pub ours_label: String,
    pub theirs_label: String,
    pub take_ours: bool,
    pub take_theirs: bool,
}

pub fn parse(text: &str) -> Vec<Segment> {
    #[derive(PartialEq)]
    enum St {
        Common,
        Ours,
        Base,
        Theirs,
    }
    let mut out = Vec::new();
    let mut common = Vec::new();
    let mut cur: Option<Conflict> = None;
    let mut st = St::Common;
    for line in text.lines() {
        match st {
            St::Common if line.starts_with("<<<<<<<") => {
                if !common.is_empty() {
                    out.push(Segment::Common(std::mem::take(&mut common)));
                }
                cur = Some(Conflict {
                    ours: Vec::new(),
                    base: Vec::new(),
                    theirs: Vec::new(),
                    ours_label: line.trim_start_matches('<').trim().to_string(),
                    theirs_label: String::new(),
                    take_ours: false,
                    take_theirs: false,
                });
                st = St::Ours;
            }
            St::Common => common.push(line.to_string()),
            St::Ours | St::Base if line.starts_with("=======") => st = St::Theirs,
            St::Ours if line.starts_with("|||||||") => st = St::Base,
            St::Ours => cur.as_mut().unwrap().ours.push(line.to_string()),
            St::Base => cur.as_mut().unwrap().base.push(line.to_string()),
            St::Theirs if line.starts_with(">>>>>>>") => {
                let mut c = cur.take().unwrap();
                c.theirs_label = line.trim_start_matches('>').trim().to_string();
                out.push(Segment::Conflict(c));
                st = St::Common;
            }
            St::Theirs => cur.as_mut().unwrap().theirs.push(line.to_string()),
        }
    }
    if let Some(c) = cur {
        // Marcadores incompletos: preserva o texto como estava.
        common.push("<<<<<<<".into());
        common.extend(c.ours);
        common.push("=======".into());
        common.extend(c.theirs);
    }
    if !common.is_empty() {
        out.push(Segment::Common(common));
    }
    out
}

pub fn conflict_count(segments: &[Segment]) -> usize {
    segments.iter().filter(|s| matches!(s, Segment::Conflict(_))).count()
}

pub fn unresolved(segments: &[Segment]) -> usize {
    segments
        .iter()
        .filter(|s| matches!(s, Segment::Conflict(c) if !c.take_ours && !c.take_theirs))
        .count()
}

/// Monta a saída: para cada conflito entram os lados escolhidos (ours antes
/// de theirs quando ambos). Conflitos sem escolha ficam vazios.
pub fn render(segments: &[Segment]) -> String {
    let mut out = String::new();
    for s in segments {
        let lines: Vec<&String> = match s {
            Segment::Common(l) => l.iter().collect(),
            Segment::Conflict(c) => {
                let mut v: Vec<&String> = Vec::new();
                if c.take_ours {
                    v.extend(&c.ours);
                }
                if c.take_theirs {
                    v.extend(&c.theirs);
                }
                v
            }
        };
        for l in lines {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "a\n<<<<<<< HEAD\nmine\n=======\ntheirs\n>>>>>>> feature\nz\n";

    #[test]
    fn parses_single_conflict() {
        let s = parse(TEXT);
        assert_eq!(s.len(), 3);
        let Segment::Conflict(c) = &s[1] else { panic!() };
        assert_eq!(c.ours, vec!["mine"]);
        assert_eq!(c.theirs, vec!["theirs"]);
        assert_eq!(c.ours_label, "HEAD");
        assert_eq!(c.theirs_label, "feature");
    }

    #[test]
    fn renders_chosen_sides_in_order() {
        let mut s = parse(TEXT);
        if let Segment::Conflict(c) = &mut s[1] {
            c.take_ours = true;
            c.take_theirs = true;
        }
        assert_eq!(render(&s), "a\nmine\ntheirs\nz\n");
        assert_eq!(unresolved(&s), 0);
    }

    #[test]
    fn diff3_base_section_is_captured() {
        let s = parse("<<<<<<< ours\nx\n||||||| base\nb\n=======\ny\n>>>>>>> theirs\n");
        let Segment::Conflict(c) = &s[0] else { panic!() };
        assert_eq!(c.base, vec!["b"]);
    }
}
