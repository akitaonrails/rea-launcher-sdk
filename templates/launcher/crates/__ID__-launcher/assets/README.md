# Window imagery

The settings window's hero image goes here (`hero.jpg`, then
`ui::HERO_JPEG`). The slot is empty: research imagery per the SDK's
`docs/STANDARDS.md` ("Give the window the game's identity"), in this order
of preference:

1. Freely licensed photos of the real subject (driver, car, track or
   place): Wikimedia Commons and similar, CC BY, CC BY-SA, CC0 or public
   domain.
2. Official art (box art, press kits) only with the owner's recorded
   decision.
3. An original illustration made for the launcher.

Never take images out of the game's files. Resize to what the window needs
(the strip is 280×640 points; a JPEG of a few hundred KB at most), record
the file, author, licence and source URL in `THIRD_PARTY.md`, and set
`ui::HERO_CREDIT` when the licence asks for attribution.
