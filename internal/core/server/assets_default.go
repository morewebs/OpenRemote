//go:build !flutterweb

package server

import "embed"

//go:embed all:dist/*
var embeddedAssets embed.FS

const embeddedDirectory = "dist"
