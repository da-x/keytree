## keytree

keytree binds a tree of key combinations to actions on Wayland. A compositor shortcut launches it, an overlay lists the next keys, and the process exits when the sequence finishes. Configured with a YAML file.

Wayland does not let a client grab keys globally. Bind the first key in the compositor so it runs `keytree --root-key …`. The overlay then takes the keyboard until the sequence finishes, Escape is pressed, or a key that is not in the tree is pressed.

The overlay is a layer-shell surface (`zwlr_layer_shell_v1`), centered on the active workspace, drawn as a rounded card with a drop shadow. The compositor has to offer that protocol. Sway, Hyprland, Niri, River, Wayfire, and KWin do. GNOME does not.


### Launching

If the configuration has a single root key, `--root-key` can be omitted. With several root keys, pass the one this shortcut should open.

```
# Sway
bindsym Menu exec keytree --root-key Menu

# Hyprland
bind = Menu, exec, keytree --root-key Menu
```

`--position` (default `%50,%50`) is the card's center inside the active workspace. A percentage is a fraction of that workspace. A plain number is a pixel offset of the card center.

`--font` is a Pango font description. The default is `normal 36`. That size is a ceiling, and the overlay will not go above 36pt. It uses the largest size that keeps the window within 80% of the active workspace height, and it will not go below 11pt.

`keytree --show-example-config` prints a sample configuration. Its root key is `C-F6`.


### Background

While many desktop environments and window managers allow binding global key combinations to actions, the definition is often accessible only via GUI or complicated commands. Also, it only works on a single level and does not let you bind key sequences. Developer IDEs allow binding key combination sequences to actions. keytree does that for the desktop.


## Features

- Binding a tree of key combinations to actions.
- On-screen display of the next keys while a sequence is in progress.


## To Do

- Implement an 'Eval' action so that actions can dynamically extend the tree, without relying on the fixed documentation.
- Allow defining a default and inheritable action for a mistyped combination on any level: Cancel, Return, Nothing, or a custom program.
- Logging cleanup.
- In each keytree node, in addition to or instead of 'next key', allow a dmenu-like selection with arrows, or a text field.
- Allow sorting the next-key help by most recently used.
- Allow a default key for the most recently used action.
- Support a JSON configuration format.


## Example configuration file

In this example, `Menu` is the prefix for every other action.

```yaml
map:
  Menu:
    title: Main actions
    map:
      c:
        title: "Chrome"
        execute: google-chrome
      r:
        title: "Reload"
        reload: ~
      s:
        title: "Sub action"
        map:
          c:
            title: "Action 1"
            execute: script
          r:
            title: "Action 2"
            execute: script
```


## License

`keytree` is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

at your option.


### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in `keytree` by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
