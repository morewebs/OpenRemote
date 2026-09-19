//go:build flutterweb

package server

import "embed"

// Generated only by tools/build; the checked-in fallback remains untouched.
//go:embed all:webdist/*
var embeddedAssets embed.FS

const embeddedDirectory = "webdist"
