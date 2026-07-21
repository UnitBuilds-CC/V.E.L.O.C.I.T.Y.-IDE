use crate::layout::LayoutBox;

#[derive(Debug, Clone)]
pub struct SvgPathCommand {
    pub cmd_type: char, // 'M', 'L', 'C', 'Z'
    pub args: Vec<f32>,
}

pub struct SvgVectorEngine;

impl SvgVectorEngine {
    pub fn parse_path_d(d: &str) -> Vec<SvgPathCommand> {
        let mut commands = Vec::new();
        let mut curr_cmd = ' ';
        let mut curr_args = Vec::new();

        for token in d.split_whitespace() {
            if let Some(ch) = token.chars().next() {
                if ch.is_alphabetic() {
                    if curr_cmd != ' ' {
                        commands.push(SvgPathCommand {
                            cmd_type: curr_cmd,
                            args: curr_args.clone(),
                        });
                        curr_args.clear();
                    }
                    curr_cmd = ch;
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
}
