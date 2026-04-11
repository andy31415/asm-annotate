//! UI color palette definitions.

use colored::Color;

/// A color palette based on the Matplotlib tab20 colormap.
///
/// This array provides a set of 20 distinct colors for use in the UI,
/// for example, to differentiate items or highlight sections.
pub const UI_PALETTE: &[Color] = &[
    Color::TrueColor {
        r: 31,
        g: 119,
        b: 180,
    }, // #1f77b4
    Color::TrueColor {
        r: 174,
        g: 199,
        b: 232,
    }, // #aec7e8
    Color::TrueColor {
        r: 255,
        g: 127,
        b: 14,
    }, // #ff7f0e
    Color::TrueColor {
        r: 255,
        g: 187,
        b: 120,
    }, // #ffbb78
    Color::TrueColor {
        r: 44,
        g: 160,
        b: 44,
    }, // #2ca02c
    Color::TrueColor {
        r: 152,
        g: 223,
        b: 138,
    }, // #98df8a
    Color::TrueColor {
        r: 214,
        g: 39,
        b: 40,
    }, // #d62728
    Color::TrueColor {
        r: 255,
        g: 152,
        b: 150,
    }, // #ff9896
    Color::TrueColor {
        r: 148,
        g: 103,
        b: 189,
    }, // #9467bd
    Color::TrueColor {
        r: 197,
        g: 176,
        b: 213,
    }, // #c5b0d5
    Color::TrueColor {
        r: 140,
        g: 86,
        b: 75,
    }, // #8c564b
    Color::TrueColor {
        r: 196,
        g: 156,
        b: 148,
    }, // #c49c94
    Color::TrueColor {
        r: 227,
        g: 119,
        b: 194,
    }, // #e377c2
    Color::TrueColor {
        r: 247,
        g: 182,
        b: 210,
    }, // #f7b6d2
    Color::TrueColor {
        r: 127,
        g: 127,
        b: 127,
    }, // #7f7f7f
    Color::TrueColor {
        r: 199,
        g: 199,
        b: 199,
    }, // #c7c7c7
    Color::TrueColor {
        r: 188,
        g: 189,
        b: 34,
    }, // #bcbd22
    Color::TrueColor {
        r: 219,
        g: 219,
        b: 141,
    }, // #dbdb8d
    Color::TrueColor {
        r: 23,
        g: 190,
        b: 207,
    }, // #17becf
    Color::TrueColor {
        r: 158,
        g: 218,
        b: 229,
    }, // #9edae5
];
