# Adeptus: Val's Screeps Bot 

My little rusty screeps bot for [Screeps: World][screeps], the JavaScript-based MMO game.

This uses the [`screeps-game-api`] bindings from the [rustyscreeps] organization.

Quickstart:
```sh
# Install CLI dependency:
cargo install cargo-screeps

# Copy the example config, and set up at least one deployment mode
cp example-screeps.toml screeps.toml
vi screeps.toml

# build tool:
cargo screeps --help
# compile the module without deploying anywhere
cargo screeps build
# compile plus deploy to the configured 'upload' mode; any section name you
# set up in your screeps.toml for different environments and servers can be used
cargo screeps deploy -m upload
# or if you've set a default mode in your configuration, simply use:
cargo screeps deploy
```

[screeps]: https://screeps.com/
[`wasm-pack`]: https://rustwasm.github.io/wasm-pack/
[`cargo-screeps`]: https://github.com/rustyscreeps/cargo-screeps/
[`screeps-game-api`]: https://github.com/rustyscreeps/screeps-game-api/
[rustyscreeps]: https://github.com/rustyscreeps/
