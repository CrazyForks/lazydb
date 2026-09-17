const MAX_DEPTH: usize = 64;
const MAX_INPUT: usize = 4 * 1024 * 1024;
const MAX_OUTPUT: usize = 8 * 1024 * 1024;

pub(super) fn mysql_geometry_wkt(bytes: &[u8]) -> Option<String> {
    if bytes.len() > MAX_INPUT {
        return None;
    }
    let input = bytes.get(4..)?;
    let mut parser = Parser {
        input,
        output: String::new(),
    };
    if parser.geometry(0, None).is_none() || !parser.input.is_empty() {
        return None;
    }
    Some(parser.output)
}

struct Parser<'a> {
    input: &'a [u8],
    output: String,
}

impl<'a> Parser<'a> {
    fn geometry(&mut self, depth: usize, expected: Option<u32>) -> Option<usize> {
        if depth > MAX_DEPTH {
            return None;
        }
        let start = self.input.len();
        let little = *self.take(1)?.first()?;
        if little > 1 {
            return None;
        }
        let little = little == 1;
        let ty = self.u32(little)?;
        if expected.is_some_and(|value| value != ty) || !(1..=7).contains(&ty) {
            return None;
        }
        let name = [
            "",
            "POINT",
            "LINESTRING",
            "POLYGON",
            "MULTIPOINT",
            "MULTILINESTRING",
            "MULTIPOLYGON",
            "GEOMETRYCOLLECTION",
        ][ty as usize];
        self.output.push_str(name);
        match ty {
            1 => self.point(little),
            2 => self.points(little),
            3 => self.polygon(little),
            4..=6 => self.multi(little, depth, ty),
            7 => self.collection(little, depth),
            _ => None,
        }?;
        Some(start - self.input.len())
    }

    fn point(&mut self, little: bool) -> Option<()> {
        self.output.push('(');
        self.coordinate(little)?;
        self.output.push(')');
        Some(())
    }

    fn points(&mut self, little: bool) -> Option<()> {
        let count = self.count(little, 16)?;
        if count == 0 {
            self.output.push_str(" EMPTY");
            return Some(());
        }
        self.output.push('(');
        for index in 0..count {
            if index > 0 {
                self.output.push(',');
            }
            self.coordinate(little)?;
        }
        self.output.push(')');
        Some(())
    }

    fn polygon(&mut self, little: bool) -> Option<()> {
        let rings = self.count(little, 4)?;
        if rings == 0 {
            self.output.push_str(" EMPTY");
            return Some(());
        }
        self.output.push('(');
        for ring in 0..rings {
            if ring > 0 {
                self.output.push(',');
            }
            let points = self.count(little, 16)?;
            self.output.push('(');
            for point in 0..points {
                if point > 0 {
                    self.output.push(',');
                }
                self.coordinate(little)?;
            }
            self.output.push(')');
        }
        self.output.push(')');
        Some(())
    }

    fn multi(&mut self, little: bool, depth: usize, ty: u32) -> Option<()> {
        let count = self.count(little, 9)?;
        if count == 0 {
            self.output.push_str(" EMPTY");
            return Some(());
        }
        self.output.push('(');
        for index in 0..count {
            if index > 0 {
                self.output.push(',');
            }
            let before = self.output.len();
            self.geometry(depth + 1, Some(ty - 3))?;
            let text = self.output[before..].to_owned();
            let prefix = ["POINT", "LINESTRING", "POLYGON"][ty as usize - 4];
            let body = text.strip_prefix(prefix)?;
            self.output.truncate(before);
            self.output.push_str(body);
        }
        self.output.push(')');
        Some(())
    }

    fn collection(&mut self, little: bool, depth: usize) -> Option<()> {
        let count = self.count(little, 9)?;
        if count == 0 {
            self.output.push_str(" EMPTY");
            return Some(());
        }
        self.output.push('(');
        for index in 0..count {
            if index > 0 {
                self.output.push(',');
            }
            self.geometry(depth + 1, None)?;
        }
        self.output.push(')');
        Some(())
    }

    fn coordinate(&mut self, little: bool) -> Option<()> {
        let x = self.f64(little)?;
        let y = self.f64(little)?;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        if self.output.len() + 48 > MAX_OUTPUT {
            return None;
        }
        self.output.push_str(&format!("{x} {y}"));
        Some(())
    }

    fn count(&mut self, little: bool, minimum_item_bytes: usize) -> Option<usize> {
        let count = usize::try_from(self.u32(little)?).ok()?;
        (count <= self.input.len().checked_div(minimum_item_bytes)?).then_some(count)
    }
    fn u32(&mut self, little: bool) -> Option<u32> {
        let bytes = self.take(4)?;
        Some(if little {
            u32::from_le_bytes(bytes.try_into().ok()?)
        } else {
            u32::from_be_bytes(bytes.try_into().ok()?)
        })
    }
    fn f64(&mut self, little: bool) -> Option<f64> {
        let bytes = self.take(8)?;
        Some(if little {
            f64::from_le_bytes(bytes.try_into().ok()?)
        } else {
            f64::from_be_bytes(bytes.try_into().ok()?)
        })
    }
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        if self.input.len() < count {
            return None;
        }
        let (head, tail) = self.input.split_at(count);
        self.input = tail;
        Some(head)
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_INPUT, mysql_geometry_wkt};

    #[test]
    fn shipping_point_matches_requested_wkt() {
        let bytes = [
            0, 0, 0, 0, 1, 1, 0, 0, 0, 0xC5, 0x20, 0xB0, 0x72, 0x68, 0x19, 0x5D, 0x40, 0x4E, 0x62,
            0x10, 0x58, 0x39, 0xF4, 0x43, 0x40,
        ];
        assert_eq!(
            mysql_geometry_wkt(&bytes).as_deref(),
            Some("POINT(116.397 39.908)")
        );
    }

    #[test]
    fn rejects_truncated_and_trailing_geometry() {
        let mut point = vec![0, 0, 0, 0, 1, 1, 0, 0, 0];
        point.extend_from_slice(&0f64.to_le_bytes());
        point.extend_from_slice(&0f64.to_le_bytes());
        assert!(mysql_geometry_wkt(&point[..point.len() - 1]).is_none());
        let mut trailing = point;
        trailing.push(1);
        assert!(mysql_geometry_wkt(&trailing).is_none());
    }

    #[test]
    fn rejects_oversized_geometry_payloads() {
        assert!(mysql_geometry_wkt(&vec![0; MAX_INPUT + 1]).is_none());
    }
}
