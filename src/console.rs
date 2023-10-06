use crossterm::style::{Print, ResetColor, SetForegroundColor};
use crossterm::{execute, style::Color};
use std::io::{self};

pub const ERROR_COLOR: Color = Color::Yellow;
pub fn pretty_print(string: &str, color: Color, newline: bool) {
    let newline = match newline {
        true => "\n",
        false => "",
    };
    execute!(
        io::stdout(),
        SetForegroundColor(color),
        Print(string),
        Print(newline),
        ResetColor
    )
    .expect("ERROR: stdout unavailable");
}
