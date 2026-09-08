use crate::sql::TextRange;

pub mod mybatis;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceSegmentKind {
    Original,
    EntityDecoded,
    Parameter,
    Synthetic,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSegment {
    pub source: TextRange,
    pub generated: TextRange,
    pub kind: SourceSegmentKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbeddedSqlUnit {
    pub source: TextRange,
    pub sql: String,
    pub segments: Vec<SourceSegment>,
    pub trusted_diagnostics: bool,
}

impl EmbeddedSqlUnit {
    /// Diagnostic spans may cover replacements; completion edits must not.
    pub fn source_diagnostic(&self, range: TextRange) -> Option<TextRange> {
        let map = |offset: usize, end: bool| {
            let segment = self
                .segments
                .iter()
                .find(|segment| {
                    if end {
                        segment.generated.start < offset && offset <= segment.generated.end
                    } else {
                        segment.generated.start <= offset && offset < segment.generated.end
                    }
                })
                .or_else(|| {
                    self.segments
                        .last()
                        .filter(|segment| offset == segment.generated.end)
                })?;
            if segment.kind == SourceSegmentKind::Original {
                Some(segment.source.start + offset - segment.generated.start)
            } else if matches!(
                segment.kind,
                SourceSegmentKind::Parameter | SourceSegmentKind::EntityDecoded
            ) {
                Some(if end || offset == segment.generated.end {
                    segment.source.end
                } else {
                    segment.source.start
                })
            } else {
                None
            }
        };
        let start = map(range.start, false)?;
        let end = if range.start == range.end {
            start
        } else {
            map(range.end, true)?
        };
        Some(TextRange::new(start, end))
    }

    pub fn source_offset(&self, generated: usize) -> Option<usize> {
        self.segments.iter().find_map(|segment| {
            if generated < segment.generated.start || generated > segment.generated.end {
                return None;
            }
            let width = segment.generated.end - segment.generated.start;
            if !matches!(
                segment.kind,
                SourceSegmentKind::Original | SourceSegmentKind::EntityDecoded
            ) {
                return None;
            }
            Some(segment.source.start + (generated - segment.generated.start).min(width))
        })
    }

    pub fn source_edit(&self, range: TextRange) -> Option<TextRange> {
        let start = self.source_offset(range.start)?;
        let end = self.source_offset(range.end)?;
        (start <= end).then_some(TextRange::new(start, end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_original_segment_in_both_directions() {
        let unit = EmbeddedSqlUnit {
            source: TextRange::new(10, 17),
            sql: "select".into(),
            segments: vec![SourceSegment {
                source: TextRange::new(10, 16),
                generated: TextRange::new(0, 6),
                kind: SourceSegmentKind::Original,
            }],
            trusted_diagnostics: true,
        };
        assert_eq!(unit.source_offset(3), Some(13));
        assert_eq!(
            unit.source_edit(TextRange::new(1, 5)),
            Some(TextRange::new(11, 15))
        );
    }

    #[test]
    fn refuses_edits_in_synthetic_segments() {
        let unit = EmbeddedSqlUnit {
            source: TextRange::new(0, 3),
            sql: "WHERE".into(),
            segments: vec![SourceSegment {
                source: TextRange::new(0, 3),
                generated: TextRange::new(0, 5),
                kind: SourceSegmentKind::Synthetic,
            }],
            trusted_diagnostics: false,
        };
        assert_eq!(unit.source_edit(TextRange::new(1, 2)), None);
    }
}
