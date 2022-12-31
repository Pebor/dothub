# Dothub
A comfortable HUB for your dotfiles :) 

*Very early alpha, be aware!*

# Usage
At your `$HOME`, `.dothub` is going to be created.
Here you can create a directory (DotFolder) named, for example `polybar`.
Inside `polybar`, you have to create a `.dothub` file for configuration (in `TOML`), the only required option is `destination` which dictates where is the location of your dot file folder.

All the options are:
| option | type | description |
| --- | --- | --- |
| start | String | Command to start the program/what you are configuring |
| kill | String | Command to kill that program |
| reload | String | Command to reaload the program. By default uses `kill` + `start` commands |
| destination | String | **REQUIRED** Destination of the dotfile folder |
| reload_on_set | Bool | If the program should restart after setting a new Dot. Default is **true** |

In your DotFolder, you can create more folders (Dots) which will be symlinked to your `destination` on `dothub set`.
You can have a `.dothub` file inside a Dot, which will be prioritized over your DotFolder's configuration.

Avaiable commands:
| command | description |
| --- | --- |
| set | Applies a dot. |
| watch | Watches a Dot and reloads on any change. *(basically brings "hot-reload" to any program)* 
| list | Lists all Dots |
| start | Runs the Dots 'start' command. |
| kill | Runs the Dots 'kill' command. |
| reload | Runs the Dots 'reload' command. If 'reload' isn't specified, it will try to use 'kill' and 'start' as an alternative. |
| run | Runs a program forked, with a different PID. (doesn't halt the terminal, you can use your WM/Compositor solutions) |
| edit | Launches $EDITOR with specified Dot. |
| profile | \*with profile commands, list below |

Profile commands:
| command | description |
| --- | --- |
| set | Applies a profile |
| list | Lists all profiles |

# Profiles
With the generation of `.dothub` in your `$HOME`, a folder called `profiles` will also be created, this isn't counted as a DotFolder.
In your `profiles`, you can define a profile as a file in `.toml` with two optional "fields".
| option | type | description |
| --- | --- | --- |
| start | array | Array of commands that should be executed on `dothub profile set` |
| dots | map | A hashmap of `DotFolder = "Dot"`, see **example** |

# Example
In your `.dothub`:
```
profiles/
  ocean.toml
  red.toml
waybar/
  .dothub
  nord/
  one_dark/
wofi/
  .dothub
  general/
  minimalist/
```
In `profiles/ocean.toml`:
```
start = [
  "notify-send 'Profile set!'",
  "notify-send 'You are now using profile ~ocean~ :)'"
]

[dots]
waybar = "nord"
wofi = "minimalist"
```
In `waybar/.dothub`:
```
start = 'dothub run waybar'
kill = 'pkill waybar'
destination = '~/.config/waybar'
```
To apply a DotProfile:
`dothub profile set ocean`

To apply a Dot:
`dothub set waybar/neon`
