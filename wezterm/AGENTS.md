# WezTerm Adapter Guide

## Debugging

- Picker actions run `projects --json` before creating the temporary picker tab. A shared-config error therefore looks like a dead binding even though the callback is registered; the adapter reports it only through logging and a toast.
- `wezterm show-keys --lua` renders `wezterm.action_callback` bindings as `EmitEvent 'user-defined-*'`. This proves key registration and config loading, not successful callback execution.
