## keytree

A daemon for X-based desktop environments that allows binding a tree of key combinations to actions. Configured with a YAML file and designed to be added to the environment startup programs. Written in Rust.


### Background

While many desktop environment and window managers allow binding global key
combinations to actions, the definition is often accessible only via GUI or
complicated commands. Also, it only works on a single-level and not letting you
bind key sequences.  In contrast, developer IDEs allow to bind key combination
*sequences* to actions. Why not allow this on the desktop level too?


## Features

- Binding a tree of key combinations to action.
- On-screen display of 'next key' help while keys are being handled.


## To Do

Various features can extensions can be implemented:

- Improve OSD window appearance.
- Implement 'Eval' action so that actions can dynamically extend the tree, without relying on the fixed documentation.
- Allow defining default and inheritable action for a mistyping of a combination on any level: Cancel, Return, Nothing, or custom program invocation.
- Logging cleanup
- In each keytree node, in addition or instead of 'next key', allow a dmenu-like capability of selection with arrows, or a text field.
- Allow to sort the 'next key' help by most-recently used.
- Allow a default key for the most-recently used.
- Support JSON configuration format.


## Example configuration file

In this example configuration file, I bind `Menu` key as the
prefix key to all other actions.

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

Some code based on Chris Duerr's unfinished [leechbar](https://github.com/chrisduerr/leechbar).


### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in `keytree` by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.A
