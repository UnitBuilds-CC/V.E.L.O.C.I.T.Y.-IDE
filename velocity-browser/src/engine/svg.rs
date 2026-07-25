#[derive(Debug, Clone)]
pub struct SvgPathCommand {
    pub cmd_type: char, // 'M', 'L', 'C', 'Z', 'H', 'V', 'S', 'Q', 'T', 'A'
    pub args: Vec<f32>,
    pub relative: bool, // lowercase commands are relative
}

/// SVG shape primitives.
#[derive(Debug, Clone)]
pub enum SvgShape {
    Rect { x: f32, y: f32, width: f32, height: f32, rx: f32, ry: f32 },
    Circle { cx: f32, cy: f32, r: f32 },
    Ellipse { cx: f32, cy: f32, rx: f32, ry: f32 },
    Line { x1: f32, y1: f32, x2: f32, y2: f32 },
    Polyline { points: Vec<(f32, f32)>, closed: bool },
    Path { commands: Vec<SvgPathCommand> },
}

/// 2D affine transform for SVG elements.
#[derive(Debug, Clone, Copy)]
pub struct SvgTransform {
    pub a: f32, pub b: f32,
    pub c: f32, pub d: f32,
    pub e: f32, pub f: f32,
}

impl SvgTransform {
    pub fn identity() -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 }
    }

    pub fn translate(tx: f32, ty: f32) -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: tx, f: ty }
    }

    pub fn scale(sx: f32, sy: f32) -> Self {
        Self { a: sx, b: 0.0, c: 0.0, d: sy, e: 0.0, f: 0.0 }
    }

    pub fn rotate(angle_rad: f32) -> Self {
        let cos = angle_rad.cos();
        let sin = angle_rad.sin();
        Self { a: cos, b: sin, c: -sin, d: cos, e: 0.0, f: 0.0 }
    }

    pub fn multiply(&self, other: &SvgTransform) -> SvgTransform {
        SvgTransform {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            e: self.a * other.e + self.c * other.f + self.e,
            f: self.b * other.e + self.d * other.f + self.f,
        }
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }
}

/// SVG path builder for constructing path data strings.
pub struct SvgPathBuilder {
    commands: Vec<SvgPathCommand>,
}

impl SvgPathBuilder {
    pub fn new() -> Self {
        Self { commands: Vec::new() }
    }

    pub fn move_to(mut self, x: f32, y: f32) -> Self {
        self.commands.push(SvgPathCommand { cmd_type: 'M', args: vec![x, y], relative: false });
        self
    }

    pub fn move_to_rel(mut self, dx: f32, dy: f32) -> Self {
        self.commands.push(SvgPathCommand { cmd_type: 'm', args: vec![dx, dy], relative: true });
        self
    }

    pub fn line_to(mut self, x: f32, y: f32) -> Self {
        self.commands.push(SvgPathCommand { cmd_type: 'L', args: vec![x, y], relative: false });
        self
    }

    pub fn line_to_rel(mut self, dx: f32, dy: f32) -> Self {
        self.commands.push(SvgPathCommand { cmd_type: 'l', args: vec![dx, dy], relative: true });
        self
    }

    pub fn horizontal(mut self, x: f32) -> Self {
        self.commands.push(SvgPathCommand { cmd_type: 'H', args: vec![x], relative: false });
        self
    }

    pub fn vertical(mut self, y: f32) -> Self {
        self.commands.push(SvgPathCommand { cmd_type: 'V', args: vec![y], relative: false });
        self
    }

    pub fn cubic_to(mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) -> Self {
        self.commands.push(SvgPathCommand { cmd_type: 'C', args: vec![x1, y1, x2, y2, x, y], relative: false });
        self
    }

    pub fn quad_to(mut self, x1: f32, y1: f32, x: f32, y: f32) -> Self {
        self.commands.push(SvgPathCommand { cmd_type: 'Q', args: vec![x1, y1, x, y], relative: false });
        self
    }

    pub fn arc(mut self, rx: f32, ry: f32, x_rotation: f32, large_arc: bool, sweep: bool, x: f32, y: f32) -> Self {
        self.commands.push(SvgPathCommand {
            cmd_type: 'A',
            args: vec![rx, ry, x_rotation, if large_arc { 1.0 } else { 0.0 }, if sweep { 1.0 } else { 0.0 }, x, y],
            relative: false,
        });
        self
    }

    pub fn close(mut self) -> Self {
        self.commands.push(SvgPathCommand { cmd_type: 'Z', args: vec![], relative: false });
        self
    }

    pub fn build(self) -> Vec<SvgPathCommand> {
        self.commands
    }

    /// Serialize to SVG path `d` attribute string.
    pub fn to_d_string(commands: &[SvgPathCommand]) -> String {
        let mut parts = Vec::new();
        for cmd in commands {
            let mut s = cmd.cmd_type.to_string();
            for (i, arg) in cmd.args.iter().enumerate() {
                if i > 0 { s.push(' '); }
                else { s.push(' '); }
                // Format: remove trailing zeros
                if *arg == arg.round() {
                    s.push_str(&format!("{}", *arg as i32));
                } else {
                    s.push_str(&format!("{:.2}", arg));
                }
            }
            parts.push(s);
        }
        parts.join(" ")
    }
}

pub struct SvgVectorEngine;

impl SvgVectorEngine {
    pub fn parse_path_d(d: &str) -> Vec<SvgPathCommand> {
        let mut commands = Vec::new();
        let mut curr_cmd = ' ';
        let mut curr_relative = false;
        let mut curr_args = Vec::new();

        for token in d.split_whitespace() {
            if let Some(ch) = token.chars().next() {
                if ch.is_alphabetic() {
                    if curr_cmd != ' ' {
                        commands.push(SvgPathCommand {
                            cmd_type: curr_cmd,
                            args: curr_args.clone(),
                            relative: curr_relative,
                        });
                        curr_args.clear();
                    }
                    curr_cmd = ch;
                    curr_relative = ch.is_lowercase();
                    let num_part = &token[1..];
                    if let Ok(val) = num_part.parse::<f32>() {
                        curr_args.push(val);
                    }
                    continue;
                }
            }
            if let Ok(val) = token.parse::<f32>() {
                curr_args.push(val);
            }
        }

        if curr_cmd != ' ' {
            commands.push(SvgPathCommand {
                cmd_type: curr_cmd,
                args: curr_args,
                relative: curr_relative,
            });
        }

        commands
    }

    pub fn compute_vector_bounds(commands: &[SvgPathCommand]) -> (f32, f32, f32, f32) {
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;

        for cmd in commands {
            let mut idx = 0;
            while idx + 1 < cmd.args.len() {
                let x = cmd.args[idx];
                let y = cmd.args[idx + 1];
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
                idx += 2;
            }
        }

        if min_x == f32::MAX {
            (0.0, 0.0, 100.0, 100.0)
        } else {
            (min_x, min_y, max_x - min_x, max_y - min_y)
        }
    }

    /// Flatten a shape into a series of line segments (for rasterization).
    pub fn flatten_shape(shape: &SvgShape, tolerance: f32) -> Vec<(f32, f32)> {
        match shape {
            SvgShape::Rect { x, y, width, height, .. } => {
                vec![(*x, *y), (*x + width, *y), (*x + width, *y + height), (*x, *y + height), (*x, *y)]
            }
            SvgShape::Circle { cx, cy, r } => {
                Self::flatten_circle(*cx, *cy, *r, tolerance)
            }
            SvgShape::Ellipse { cx, cy, rx, ry } => {
                Self::flatten_ellipse(*cx, *cy, *rx, *ry, tolerance)
            }
            SvgShape::Line { x1, y1, x2, y2 } => {
                vec![(*x1, *y1), (*x2, *y2)]
            }
            SvgShape::Polyline { points, closed } => {
                let mut pts = points.clone();
                if *closed && !pts.is_empty() {
                    pts.push(pts[0]);
                }
                pts
            }
            SvgShape::Path { commands } => {
                Self::flatten_path(commands, tolerance)
            }
        }
    }

    fn flatten_circle(cx: f32, cy: f32, r: f32, tolerance: f32) -> Vec<(f32, f32)> {
        Self::flatten_ellipse(cx, cy, r, r, tolerance)
    }

    fn flatten_ellipse(cx: f32, cy: f32, rx: f32, ry: f32, tolerance: f32) -> Vec<(f32, f32)> {
        let max_r = rx.max(ry);
        let segments = ((std::f32::consts::TAU * max_r) / tolerance).ceil().max(8.0) as usize;
        let mut points = Vec::with_capacity(segments + 1);
        for i in 0..=segments {
            let angle = std::f32::consts::TAU * i as f32 / segments as f32;
            points.push((cx + rx * angle.cos(), cy + ry * angle.sin()));
        }
        points
    }

    fn flatten_path(commands: &[SvgPathCommand], _tolerance: f32) -> Vec<(f32, f32)> {
        let mut points = Vec::new();
        let mut cur_x = 0.0f32;
        let mut cur_y = 0.0f32;
        for cmd in commands {
            match cmd.cmd_type.to_ascii_uppercase() {
                'M' if cmd.args.len() >= 2 => {
                    if cmd.relative {
                        cur_x += cmd.args[0];
                        cur_y += cmd.args[1];
                    } else {
                        cur_x = cmd.args[0];
                        cur_y = cmd.args[1];
                    }
                    points.push((cur_x, cur_y));
                }
                'L' if cmd.args.len() >= 2 => {
                    if cmd.relative {
                        cur_x += cmd.args[0];
                        cur_y += cmd.args[1];
                    } else {
                        cur_x = cmd.args[0];
                        cur_y = cmd.args[1];
                    }
                    points.push((cur_x, cur_y));
                }
                'H' if !cmd.args.is_empty() => {
                    if cmd.relative { cur_x += cmd.args[0]; } else { cur_x = cmd.args[0]; }
                    points.push((cur_x, cur_y));
                }
                'V' if !cmd.args.is_empty() => {
                    if cmd.relative { cur_y += cmd.args[0]; } else { cur_y = cmd.args[0]; }
                    points.push((cur_x, cur_y));
                }
                'C' if cmd.args.len() >= 6 => {
                    // Simplified: just add endpoints
                    if cmd.relative {
                        cur_x += cmd.args[4];
                        cur_y += cmd.args[5];
                    } else {
                        cur_x = cmd.args[4];
                        cur_y = cmd.args[5];
                    }
                    points.push((cur_x, cur_y));
                }
                'Q' if cmd.args.len() >= 4 => {
                    if cmd.relative {
                        cur_x += cmd.args[2];
                        cur_y += cmd.args[3];
                    } else {
                        cur_x = cmd.args[2];
                        cur_y = cmd.args[3];
                    }
                    points.push((cur_x, cur_y));
                }
                'Z' => {
                    if let Some(&first) = points.first() {
                        cur_x = first.0;
                        cur_y = first.1;
                        points.push((cur_x, cur_y));
                    }
                }
                _ => {}
            }
        }
        points
    }

    /// Create a rect shape.
    pub fn make_rect(x: f32, y: f32, w: f32, h: f32) -> SvgShape {
        SvgShape::Rect { x, y, width: w, height: h, rx: 0.0, ry: 0.0 }
    }

    /// Create a circle shape.
    pub fn make_circle(cx: f32, cy: f32, r: f32) -> SvgShape {
        SvgShape::Circle { cx, cy, r }
    }

    /// Create a line shape.
    pub fn make_line(x1: f32, y1: f32, x2: f32, y2: f32) -> SvgShape {
        SvgShape::Line { x1, y1, x2, y2 }
    }

    /// Compute the total path length (approximate).
    pub fn path_length(commands: &[SvgPathCommand]) -> f32 {
        let points = Self::flatten_path(commands, 1.0);
        let mut length = 0.0f32;
        for i in 1..points.len() {
            let dx = points[i].0 - points[i - 1].0;
            let dy = points[i].1 - points[i - 1].1;
            length += (dx * dx + dy * dy).sqrt();
        }
        length
    }
}
