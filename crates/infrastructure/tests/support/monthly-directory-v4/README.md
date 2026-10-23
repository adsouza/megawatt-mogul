# Frozen directory format 4 fixture

This small charged-battery snapshot is the binary-directory equivalent of the retained
`portable-saves/schema1-charged-battery.mgsave` format-2 fixture. It preserves the same metadata,
canonical portable state, journal, region and future continuation. The directory contains a
compressed text manifest and one compressed monthly binary chart file. Do not regenerate it to
clear compatibility failures; preserve the reader or add an explicit upcaster.

This is a permanent compatibility test fixture, not personal save data or recovery tooling.
