# Open source credits

feather-chat is built with and ships alongside open source work. The project's
own code is licensed under [AGPL-3.0](LICENSE); the assets and libraries below
remain under their respective licenses.

## Fonts

### Aldrich

Copyright (c) 2011, Matthew Desmond
(<http://www.madtype.com> | <mattdesmond@gmail.com>), with Reserved Font Name
Aldrich.

This Font Software is licensed under the SIL Open Font License, Version 1.1.
The full license text is in
[`client/assets/fonts/Aldrich/OFL.txt`](client/assets/fonts/Aldrich/OFL.txt)
and is also available at <https://openfontlicense.org>.

## Icons

### Phosphor Icons

Copyright (c) 2023 Phosphor Icons (<https://phosphoricons.com>).

All UI icons in [`client/assets/icons/`](client/assets/icons/) come from the
Phosphor Icons set: arrow-left, caret-down, chats-teardrop, compass, copy,
dots-three, ear, ear-slash, eye, eye-slash, gear-six, hash, home, microphone,
microphone-slash, minus, phone, phone-slash, plus, server, sidebar-simple,
speaker-high, square, and x.

They are licensed under the MIT License. The full license text is in
[`client/assets/icons/LICENSE`](client/assets/icons/LICENSE) and is also
available at
<https://github.com/phosphor-icons/core/blob/main/LICENSE>.

## Libraries

Direct dependencies of the client and server. Transitive dependencies are
covered by their own licenses in the respective lock files
([`client/Cargo.lock`](client/Cargo.lock), [`server/go.sum`](server/go.sum)).

### Client (Rust)

| Crate | License | Upstream |
| ----- | ------- | -------- |
| [gpui], [gpui_platform] | Apache-2.0 | [Zed Industries](https://github.com/zed-industries/zed) |
| [serde_json] | MIT OR Apache-2.0 | [serde-rs](https://github.com/serde-rs/json) |
| [async-channel] | MIT OR Apache-2.0 | [smol-rs](https://github.com/smol-rs/async-channel) |
| [unicode-segmentation] | MIT OR Apache-2.0 | [unicode-rs](https://github.com/unicode-rs/unicode-segmentation) |
| [rand] | MIT OR Apache-2.0 | [rust-random](https://github.com/rust-random/rand) |
| [cpal] | Apache-2.0 | [RustAudio](https://github.com/rustaudio/cpal) |
| [native-theme] | MIT OR Apache-2.0 OR 0BSD | [tiborgats](https://github.com/tiborgats/native-theme) |
| [opus-rs] | BSD-3-Clause | [restsend](https://github.com/restsend/opus-rs) |
| [rtrb] | MIT OR Apache-2.0 | [mgeier](https://github.com/mgeier/rtrb) |
| [keyring] | MIT OR Apache-2.0 | [hwchen](https://github.com/hwchen/keyring-rs) |
| [image] (Linux/BSD) | MIT OR Apache-2.0 | [image-rs](https://github.com/image-rs/image) |
| [objc2], [objc2-app-kit], [objc2-foundation] (macOS) | MIT | [madsmtm](https://github.com/madsmtm/objc2) |
| [embed-resource] (build) | MIT | [nabijaczleweli](https://github.com/nabijaczleweli/rust-embed-resource) |

[gpui]: https://github.com/zed-industries/zed/tree/main/crates/gpui
[gpui_platform]: https://github.com/zed-industries/zed/tree/main/crates/gpui_platform
[serde_json]: https://github.com/serde-rs/json
[async-channel]: https://github.com/smol-rs/async-channel
[unicode-segmentation]: https://github.com/unicode-rs/unicode-segmentation
[rand]: https://github.com/rust-random/rand
[cpal]: https://github.com/rustaudio/cpal
[native-theme]: https://github.com/tiborgats/native-theme
[opus-rs]: https://github.com/restsend/opus-rs
[rtrb]: https://github.com/mgeier/rtrb
[keyring]: https://github.com/hwchen/keyring-rs
[image]: https://github.com/image-rs/image
[objc2]: https://github.com/madsmtm/objc2
[objc2-app-kit]: https://github.com/madsmtm/objc2
[objc2-foundation]: https://github.com/madsmtm/objc2
[embed-resource]: https://github.com/nabijaczleweli/rust-embed-resource

`opus-rs` compiles and links [libopus], licensed BSD-3-Clause by the
Xiph.Org Foundation and contributors.

[libopus]: https://github.com/xiph/opus

### Server (Go)

| Module | License | Upstream |
| ------ | ------- | -------- |
| [golang-migrate/migrate/v4] | MIT | [golang-migrate](https://github.com/golang-migrate/migrate) |
| [jackc/pgx/v5] | MIT | [jackc](https://github.com/jackc/pgx) |
| [joho/godotenv] | MIT | [joho](https://github.com/joho/godotenv) |
| [mongo-driver/v2] | Apache-2.0 | [mongodb](https://github.com/mongodb/mongo-go-driver) |
| [golang.org/x/crypto] | BSD-3-Clause | [Go Authors](https://go.googlesource.com/crypto) |

[golang-migrate/migrate/v4]: https://github.com/golang-migrate/migrate
[jackc/pgx/v5]: https://github.com/jackc/pgx
[joho/godotenv]: https://github.com/joho/godotenv
[mongo-driver/v2]: https://github.com/mongodb/mongo-go-driver
[golang.org/x/crypto]: https://go.googlesource.com/crypto
