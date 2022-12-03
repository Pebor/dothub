# Dothub
A comfortable HUB for your dotfiles :) 

*Very early alpha, be aware!*

# Usage
At your `$HOME`, `.dothub` is going to be created.
Here you can create a directory (DotFolder) named, for example `neovim`.
Inside `neovim`, you have to create a `.dothub` file for configuration (in `TOML`), the only required option is `destination` which dictates where is the location of your dot file folder.

All the options are:
| option | type | description |
| --- | --- | --- |
| start | String | Command to start the program/what you are configuring |
| kill | String | Command to kill that program |
| reload | String | Command to reaload the program. By default uses `kill` + `start` commands |
| destination | String | **REQUIRED** Destination of the dotfile folder |
| reload_on_set | Bool | If the program should restart after setting a new Dot. Default is **true** |

In your DotFolder, you can create more folders (Dots) which will be symlinked to your `destination` on `set`.
You can have a `.dothub` file inside a Dot, which will be prioritized over your DotFolder's configuration.

# Example
In .dothub:
```
waybar/
  .dothub
  neon/
  the_cool_one/
```
In `waybar/.dothub'
```
start = 'hyprctl dispatch exec waybar'
kill = 'pkill waybar'
destination = '~/.config/waybar'
```
To apply a Dot:
`dothub set waybar neon`
