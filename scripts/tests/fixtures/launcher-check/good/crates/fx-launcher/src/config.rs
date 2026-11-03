pub const OPTIONS: &[OptionSpec] = &[
    OptionSpec::new("game", Kind::Path, "Game folder", "The game."),
    OptionSpec::new(
        "look",
        Kind::Choice(LOOKS),
        "Look",
        "The look.",
    ),
    OptionSpec::new("frame_cap", Kind::Toggle, "Frame cap", "Caps it."),
];

#[cfg(test)]
mod tests {
    #[test]
    fn game_and_look() {
        let _ = ["game", "look"];
    }
}
