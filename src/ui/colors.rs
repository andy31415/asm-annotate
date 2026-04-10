use colored::Color;

pub const UI_PALETTE: &[Color] = &[
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::BrightRed,
    Color::BrightGreen,
    Color::BrightYellow,
    Color::BrightBlue,
    Color::BrightMagenta,
    Color::BrightCyan,
    // Add more colors
    Color::TrueColor { r: 255, g: 165, b: 0 }, // Orange
    Color::TrueColor { r: 128, g: 0, b: 128 }, // Purple
    Color::TrueColor { r: 0, g: 128, b: 128 }, // Teal
    Color::TrueColor { r: 255, g: 192, b: 203 }, // Pink
    Color::TrueColor { r: 165, g: 42, b: 42 },  // Brown
    Color::TrueColor { r: 255, g: 215, b: 0 },  // Gold
];
