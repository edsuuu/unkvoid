//! O som do sistema sem os aplicativos silenciados e sem o nosso, no Linux.
//!
//! O PulseAudio (e o `pipewire-pulse`, que fala a mesma língua) só entrega o monitor de
//! um sink inteiro: não existe "tudo menos este processo". Então o filtro é um sink:
//! um `module-combine-sink` que desagua na saída padrão, para onde vão todos os
//! streams menos os silenciados e os nossos, e a captura lê o monitor dele. Quem fica
//! de fora continua tocando direto na saída padrão — continua sendo ouvido, só não sobe.
//!
//! O `pactl subscribe` avisa de cada stream que nasce durante a transmissão, e ele é
//! movido na hora. Descarregar o módulo no fim devolve todo mundo à saída padrão.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use crate::CaptureConfig;

const SINK: &str = "unkvoid_share";

/// O device que o `pulsesrc` lê enquanto o sink está de pé.
pub const MONITOR: &str = "unkvoid_share.monitor";

/// Os daemons de som: os streams internos deles (o próprio combine, loopbacks) nunca
/// entram no nosso sink, senão o som dá a volta e realimenta.
const DAEMONS: &[&str] = &["pulseaudio", "pipewire", "pipewire-pulse", "wireplumber"];

/// O sink de pé: descarregá-lo é o `Drop`.
pub struct SharedSink {
    module: u32,
    subscribe: Child,
}

impl SharedSink {
    /// Sobe o sink na frente da saída padrão e move para ele o que já toca.
    ///
    /// ponytail: a saída padrão é a do momento; trocar de fone no meio da transmissão
    /// deixa o combine preso à antiga até o próximo `start`.
    pub fn open(mute_listed_apps: bool) -> Result<Self, String> {
        let default = pactl(&["get-default-sink"])?.trim().to_string();

        if default.is_empty() {
            return Err("sem saída de som padrão".into());
        }

        let module = pactl(&[
            "load-module",
            "module-combine-sink",
            &format!("sink_name={SINK}"),
            &format!("slaves={default}"),
            "sink_properties=device.description=Unkvoid",
        ])?
        .trim()
        .parse()
        .map_err(|_| "o pactl não devolveu o índice do módulo".to_string())?;

        let subscribe = Command::new("pactl")
            .env("LC_ALL", "C")
            .arg("subscribe")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("pactl subscribe não abriu ({error})"))?;

        let mut sink = Self { module, subscribe };
        let muted: Vec<String> = if mute_listed_apps {
            CaptureConfig::MUTED_EXECUTABLES.iter().map(|name| name.trim_end_matches(".exe").to_ascii_lowercase()).collect()
        } else {
            Vec::new()
        };

        route(module, &muted);

        if let Some(events) = sink.subscribe.stdout.take() {
            std::thread::spawn(move || {
                for line in BufReader::new(events).lines().map_while(Result::ok) {
                    if line.contains("'new' on sink-input") {
                        route(module, &muted);
                    }
                }
            });
        }

        Ok(sink)
    }
}

impl Drop for SharedSink {
    fn drop(&mut self) {
        let _ = self.subscribe.kill();
        let _ = self.subscribe.wait();
        let _ = pactl(&["unload-module", &self.module.to_string()]);
    }
}

/// Move para o sink cada stream que deve subir. O que já está lá é um no-op.
fn route(module: u32, muted: &[String]) {
    let Ok(listing) = pactl(&["list", "sink-inputs"]) else {
        return;
    };

    for index in targets(&listing, module, muted, std::process::id(), parent_of) {
        if let Err(error) = pactl(&["move-sink-input", &index.to_string(), SINK]) {
            tracing::debug!(error = %error, index, "captura: stream não foi para o sink");
        }
    }
}

/// Os índices que entram na transmissão: todo stream de processo de fora, menos os
/// silenciados, os nossos (o app e os `gst-launch` filhos dele) e os dos daemons.
fn targets(
    listing: &str,
    module: u32,
    muted: &[String],
    own: u32,
    parent_of: impl Fn(u32) -> Option<u32>,
) -> Vec<u32> {
    listing
        .split("Sink Input #")
        .skip(1)
        .filter_map(|block| {
            let index: u32 = block.lines().next()?.trim().parse().ok()?;
            let field = |key: &str| {
                block
                    .lines()
                    .find_map(|line| line.trim().strip_prefix(key))
                    .map(|rest| rest.trim().trim_matches('"').to_ascii_lowercase())
            };
            let pid: u32 = field("application.process.id = ")?.parse().ok()?;
            let binary = field("application.process.binary = ")?;
            let mut names = std::iter::once(binary).chain(field("application.name = "));
            let ours = pid == own || parent_of(pid) == Some(own);
            let owner = field("Owner Module:").and_then(|owner| owner.parse::<u32>().ok());

            if ours || owner == Some(module) {
                return None;
            }

            let excluded = names.any(|name| muted.contains(&name) || DAEMONS.contains(&name.as_str()));

            (!excluded).then_some(index)
        })
        .collect()
}

/// O pai de um processo, pelo `/proc`: é como um `gst-launch` nosso se revela nosso.
fn parent_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_name = stat.rsplit_once(')')?.1;

    after_name.split_whitespace().nth(1)?.parse().ok()
}

/// Um `pactl` em inglês: a saída é localizada, e o parser lê "Sink Input #".
fn pactl(args: &[&str]) -> Result<String, String> {
    let output = Command::new("pactl")
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .map_err(|error| format!("pactl não abriu ({error}); instale pulseaudio-utils"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "\
Sink Input #10
\tDriver: protocol-native.c
\tOwner Module: 7
\tProperties:
\t\tapplication.name = \"Discord\"
\t\tapplication.process.id = \"100\"
\t\tapplication.process.binary = \"Discord\"

Sink Input #11
\tDriver: protocol-native.c
\tOwner Module: 7
\tProperties:
\t\tapplication.name = \"Counter-Strike 2\"
\t\tapplication.process.id = \"200\"
\t\tapplication.process.binary = \"cs2\"

Sink Input #12
\tDriver: protocol-native.c
\tOwner Module: 7
\tProperties:
\t\tapplication.name = \"gst-launch-1.0\"
\t\tapplication.process.id = \"300\"
\t\tapplication.process.binary = \"gst-launch-1.0\"

Sink Input #13
\tDriver: module-combine-sink.c
\tOwner Module: 42
\tProperties:
\t\tapplication.name = \"Simultaneous output\"
\t\tapplication.process.id = \"1\"
\t\tapplication.process.binary = \"pulseaudio\"

Sink Input #14
\tDriver: protocol-native.c
\tOwner Module: 7
\tProperties:
\t\tapplication.name = \"vesktop\"
\t\tapplication.process.id = \"400\"
\t\tapplication.process.binary = \"electron\"

Sink Input #15
\tDriver: protocol-native.c
\tOwner Module: 7
\tProperties:
\t\tapplication.name = \"Firefox\"
\t\tapplication.process.id = \"500\"
\t\tapplication.process.binary = \"firefox\"
";

    #[test]
    fn only_the_game_and_the_browser_go_up() {
        let parent_of = |pid: u32| (pid == 300).then_some(999);
        let muted = ["discord".to_string(), "vesktop".to_string()];

        assert_eq!(targets(LISTING, 42, &muted, 999, parent_of), vec![11, 15]);
    }

    #[test]
    fn without_the_flag_the_call_goes_up_but_never_ourselves() {
        let parent_of = |pid: u32| (pid == 300).then_some(999);

        assert_eq!(targets(LISTING, 42, &[], 999, parent_of), vec![10, 11, 14, 15]);
    }
}

/// Contra um daemon de verdade: `cargo test -p capture -- --ignored` numa máquina (ou
/// container) com `pulseaudio` no ar. Um `paplay` copiado como `Discord` e outro como
/// `cs2` tocam ao mesmo tempo; só o jogo tem de acabar no nosso sink.
#[cfg(test)]
mod daemon {
    use super::*;

    #[test]
    #[ignore]
    fn the_game_moves_to_our_sink_and_the_call_stays_on_the_default() {
        let default = pactl(&["get-default-sink"]).expect("pulseaudio no ar").trim().to_string();
        let which = Command::new("which").arg("paplay").output().unwrap().stdout;
        let paplay = std::fs::read(String::from_utf8(which).unwrap().trim()).unwrap();
        let scratch = std::env::temp_dir().join("unkvoid-audio-test");
        std::fs::create_dir_all(&scratch).unwrap();

        let mut players = Vec::new();

        for name in ["Discord", "cs2"] {
            let path = scratch.join(name);
            std::fs::write(&path, &paplay).unwrap();
            std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
            players.push((name, Command::new(&path).args(["--raw", "/dev/zero"]).stdin(Stdio::null()).stderr(Stdio::null()).spawn().unwrap()));
        }

        std::thread::sleep(std::time::Duration::from_millis(500));

        let sink = SharedSink::open(true).expect("o combine sink sobe");

        std::thread::sleep(std::time::Duration::from_millis(500));

        let listing = pactl(&["list", "sink-inputs"]).unwrap();
        let sinks: Vec<String> = pactl(&["list", "sinks", "short"]).unwrap().lines().map(|line| line.split('\t').take(2).map(str::to_string).collect::<Vec<_>>().join(" ")).collect();
        let sink_of = |binary: &str| -> String {
            let block = listing.split("Sink Input #").find(|block| block.contains(&format!("application.process.binary = \"{binary}\""))).expect(binary);
            let index = block.lines().find_map(|line| line.trim().strip_prefix("Sink: ")).unwrap().trim();

            sinks.iter().find(|sink| sink.starts_with(&format!("{index} "))).unwrap().clone()
        };

        assert!(sink_of("cs2").ends_with(SINK), "jogo em {}", sink_of("cs2"));
        assert!(sink_of("Discord").ends_with(&default), "chamada em {}", sink_of("Discord"));

        drop(sink);

        std::thread::sleep(std::time::Duration::from_millis(300));

        assert!(!pactl(&["list", "sinks", "short"]).unwrap().contains(SINK), "o sink sumiu no fim");

        for (_, mut player) in players {
            let _ = player.kill();
        }
    }
}
