# Scratch dir for testenv-* recipes — isolated config (XDG_CONFIG_HOME) and
# data/db (XDG_DATA_HOME) so testing never touches the real ~/.config/mynd
# or ~/.local/share/mynd.
testenv_dir := justfile_directory() / ".testenv"

# Scaffold a global config + .mynd.toml under .testenv, isolated from your real setup
testenv-init:
  XDG_CONFIG_HOME={{ testenv_dir }}/config XDG_DATA_HOME={{ testenv_dir }}/data cargo run -- init

# Run `mynd up` against the .testenv config/db instead of your real one
testenv-up:
  XDG_CONFIG_HOME={{ testenv_dir }}/config XDG_DATA_HOME={{ testenv_dir }}/data just run up

# Run `mynd status` against the .testenv config/db instead of your real one
testenv-status:
  XDG_CONFIG_HOME={{ testenv_dir }}/config XDG_DATA_HOME={{ testenv_dir }}/data just run status

# Wipe the .testenv scratch dir
testenv-clean:
  rm -rf {{ testenv_dir }}
