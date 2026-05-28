pub async fn run() -> Result<String, Box<dyn std::error::Error>> {
    let schema = serde_json::json!({
        "name": "nixman",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "NixOS configuration manager CLI",
        "commands": {
            "status": {
                "description": "Quick workspace and system overview",
                "args": [],
                "flags": []
            },
            "check": {
                "description": "Validate configuration without building",
                "args": [],
                "flags": []
            },
            "doctor": {
                "description": "Run system health checks",
                "args": [],
                "flags": []
            },
            "diff": {
                "description": "Show uncommitted config changes",
                "args": [],
                "flags": [{"name": "--staged", "description": "Show only file-backed staged changes"}]
            },
            "rebuild": {
                "description": "Run nixos-rebuild",
                "args": [{"name": "mode", "type": "string", "required": false, "default": "switch", "values": ["switch", "boot", "test", "build"]}],
                "flags": []
            },
            "option": {
                "description": "Get, set, search NixOS options",
                "subcommands": {
                    "get": {
                        "description": "Get current value of an option",
                        "args": [{"name": "path", "type": "option-path", "required": true}],
                        "flags": []
                    },
                    "set": {
                        "description": "Set an option value",
                        "args": [
                            {"name": "path", "type": "option-path", "required": true},
                            {"name": "value", "type": "nix-value", "required": false}
                        ],
                        "flags": [
                            {"name": "--stdin", "description": "Read value from stdin"},
                            {"name": "--dry-run", "description": "Show what would change"},
                            {"name": "--stage", "description": "Stage change instead of applying"}
                        ]
                    },
                    "remove": {
                        "description": "Remove an option from config",
                        "args": [{"name": "path", "type": "option-path", "required": true}],
                        "flags": [{"name": "--dry-run", "description": "Show what would change"}]
                    },
                    "search": {
                        "description": "Search available NixOS options",
                        "args": [{"name": "query", "type": "string", "required": true}],
                        "flags": [{"name": "--limit", "type": "integer", "description": "Maximum number of results", "default": "20"}]
                    },
                    "browse": {
                        "description": "Browse options under a prefix",
                        "args": [{"name": "prefix", "type": "option-path", "required": false}],
                        "flags": [{"name": "--limit", "type": "integer", "description": "Maximum number of results", "default": "100"}]
                    },
                    "show": {
                        "description": "Show full details of a specific option",
                        "args": [{"name": "path", "type": "option-path", "required": true}],
                        "flags": []
                    }
                }
            },
            "packages": {
                "description": "Search and manage packages",
                "subcommands": {
                    "list": {
                        "description": "List packages declared in NixOS configuration",
                        "args": [],
                        "flags": []
                    },
                    "search": {
                        "description": "Search nixpkgs for packages matching a query",
                        "args": [{"name": "query", "type": "string", "required": true}],
                        "flags": []
                    },
                    "add": {
                        "description": "Add a package to environment.systemPackages",
                        "args": [{"name": "name", "type": "package-name", "required": true}],
                        "flags": [
                            {"name": "--no-verify", "description": "Skip package name verification against nixpkgs"},
                            {"name": "--file", "type": "path", "description": "Target file for the package"},
                            {"name": "--dry-run", "description": "Show what would change without writing"}
                        ]
                    },
                    "remove": {
                        "description": "Remove a package from environment.systemPackages",
                        "args": [{"name": "name", "type": "package-name", "required": true}],
                        "flags": [
                            {"name": "--file", "type": "path", "description": "Target file for the package"},
                            {"name": "--dry-run", "description": "Show what would change without writing"}
                        ]
                    }
                }
            },
            "services": {
                "description": "List and control systemd services",
                "subcommands": {
                    "list": {
                        "description": "List all systemd service units",
                        "args": [],
                        "flags": []
                    },
                    "get": {
                        "description": "Show status of a specific service",
                        "args": [{"name": "unit", "type": "string", "required": true}],
                        "flags": []
                    },
                    "start": {
                        "description": "Start a service",
                        "args": [{"name": "unit", "type": "string", "required": true}],
                        "flags": []
                    },
                    "stop": {
                        "description": "Stop a service",
                        "args": [{"name": "unit", "type": "string", "required": true}],
                        "flags": []
                    },
                    "restart": {
                        "description": "Restart a service",
                        "args": [{"name": "unit", "type": "string", "required": true}],
                        "flags": []
                    },
                    "logs": {
                        "description": "Show journal logs for a service",
                        "args": [{"name": "unit", "type": "string", "required": true}],
                        "flags": [{"name": "-n/--lines", "type": "integer", "description": "Number of log lines", "default": "50"}]
                    }
                }
            },
            "flake": {
                "description": "Manage flake inputs",
                "subcommands": {
                    "list": {
                        "description": "List all flake inputs",
                        "args": [],
                        "flags": []
                    },
                    "show": {
                        "description": "Show the current flake metadata",
                        "args": [],
                        "flags": []
                    },
                    "update": {
                        "description": "Update one or all flake inputs",
                        "args": [{"name": "input", "type": "string", "required": false}],
                        "flags": []
                    }
                }
            },
            "generations": {
                "description": "List, diff, rollback generations",
                "subcommands": {
                    "list": {
                        "description": "List all system generations",
                        "args": [],
                        "flags": []
                    },
                    "diff": {
                        "description": "Show package diff between two generations",
                        "args": [
                            {"name": "from", "type": "integer", "required": true, "description": "Older generation number"},
                            {"name": "to", "type": "integer", "required": true, "description": "Newer generation number"}
                        ],
                        "flags": []
                    },
                    "rollback": {
                        "description": "Roll back to a previous generation",
                        "args": [{"name": "number", "type": "integer", "required": true, "description": "Generation number to activate"}],
                        "flags": []
                    },
                    "gc": {
                        "description": "Delete old generations and run garbage collection",
                        "args": [],
                        "flags": [{"name": "--keep", "type": "integer", "description": "Number of most-recent generations to keep"}]
                    }
                }
            },
            "pending": {
                "description": "Manage staged changes",
                "subcommands": {
                    "list": {
                        "description": "List all staged changes",
                        "args": [],
                        "flags": []
                    },
                    "apply": {
                        "description": "Apply all staged changes to disk",
                        "args": [],
                        "flags": []
                    },
                    "discard": {
                        "description": "Discard all staged changes",
                        "args": [],
                        "flags": []
                    }
                }
            },
            "intent": {
                "description": "Propose, review, apply, or discard configuration change plans",
                "subcommands": {
                    "propose": {
                        "description": "Propose changes and get a validated plan",
                        "args": [],
                        "flags": [
                            {"name": "--set", "type": "path=value", "repeatable": true, "description": "Options to set (format: path=value)"},
                            {"name": "--add-package", "type": "string", "repeatable": true, "description": "Packages to add to environment.systemPackages"},
                            {"name": "--remove-package", "type": "string", "repeatable": true, "description": "Packages to remove from environment.systemPackages"}
                        ]
                    },
                    "show": {
                        "description": "Show the last proposed plan",
                        "args": [],
                        "flags": []
                    },
                    "apply": {
                        "description": "Apply the last proposed plan",
                        "args": [],
                        "flags": []
                    },
                    "discard": {
                        "description": "Discard the current plan",
                        "args": [],
                        "flags": []
                    }
                }
            },
            "workspace": {
                "description": "Workspace detection and setup",
                "subcommands": {
                    "detect": {
                        "description": "Auto-detect NixOS configuration location",
                        "args": [],
                        "flags": []
                    },
                    "wizard": {
                        "description": "Run first-time setup wizard",
                        "args": [],
                        "flags": [{"name": "--path", "type": "path", "description": "Target directory for new workspace"}]
                    }
                }
            },
            "schema": {
                "description": "Output command schema for agents",
                "args": [],
                "flags": []
            },
            "try": {
                "description": "Temporary config changes with auto-revert",
                "subcommands": {
                    "apply": {
                        "description": "Apply temporary changes with auto-revert timeout",
                        "args": [],
                        "flags": [
                            {"name": "--set", "type": "path=value", "repeatable": true, "description": "Options to set temporarily"},
                            {"name": "--timeout", "type": "integer", "default": "120", "description": "Seconds before auto-revert"}
                        ]
                    },
                    "confirm": {
                        "description": "Confirm temporary changes (make permanent)",
                        "args": [],
                        "flags": []
                    }
                }
            },
            "hm": {
                "description": "Manage Home Manager user configuration",
                "subcommands": {
                    "status": {"description": "Show Home Manager workspace status", "args": [], "flags": []},
                    "option": {"description": "Get, set, search Home Manager options", "args": [], "flags": []},
                    "packages": {"description": "Search and manage Home Manager packages", "args": [], "flags": []},
                    "rebuild": {
                        "description": "Rebuild Home Manager configuration",
                        "args": [{"name": "mode", "type": "string", "required": true, "values": ["switch", "build", "boot", "test"]}],
                        "flags": [
                            {"name": "--explain", "description": "Explain errors in plain English"},
                            {"name": "--rollback-on-fail", "description": "Roll back if build fails"}
                        ]
                    }
                }
            },
            "explain": {
                "description": "Explain a Nix error in plain English",
                "args": [{"name": "error", "type": "string", "required": false}],
                "flags": [{"name": "--stdin", "description": "Read error from stdin"}]
            },
            "migrate": {
                "description": "Detect and fix deprecated options",
                "args": [],
                "flags": [{"name": "--fix", "description": "Auto-fix renameable options"}]
            },
            "history": {
                "description": "Enhanced generation history with change context",
                "args": [],
                "flags": [{"name": "--diff", "description": "Show package changes between generations"}]
            }
        },
        "global_flags": [
            {"name": "--workspace", "type": "path", "description": "Path to NixOS config workspace (auto-detects if omitted)"},
            {"name": "-q/--quiet", "description": "Suppress informational messages"},
            {"name": "-v/--verbose", "description": "Increase verbosity (repeat for trace)"},
            {"name": "-y/--yes", "description": "Skip confirmation prompts"},
            {"name": "--version", "type": "bool", "description": "Show version information"}
        ]
    });

    Ok(serde_json::to_string_pretty(&schema)?)
}
