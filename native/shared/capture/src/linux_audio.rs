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
    /// A saída padrão de quando o sink subiu, para onde os streams voltam no fim.
    default: String,
}

impl SharedSink {
    /// Sobe o sink na frente da saída padrão e move para ele o que já toca.
    ///
    /// `adjust_time=0` desliga o relógio que acerta a taxa entre os destinos — com um destino
    /// só não há o que acertar. Ligado, o PulseAudio 16 cai (`Assertion 'u->time_event == e'`
    /// no `module-combine-sink`) quando um stream com outra taxa (o jogo em 44,1 kHz) entra no
    /// sink recém-criado: compartilhar a tela derrubava o som da máquina. Quem não conhece o
    /// argumento recusa o carregamento, e aí ele sobe sem.
    ///
    /// ponytail: a saída padrão é a do momento; trocar de fone no meio da transmissão
    /// deixa o combine preso à antiga até o próximo `start`.
    pub fn open(mute_listed_apps: bool) -> Result<Self, String> {
        clear_leftovers();

        let default = pactl(&["get-default-sink"])?.trim().to_string();

        if default.is_empty() {
            return Err("sem saída de som padrão".into());
        }

        let (name, slaves) = (format!("sink_name={SINK}"), format!("slaves={default}"));
        let arguments = ["load-module", "module-combine-sink", &name, &slaves, "sink_properties=device.description=Unkvoid"];
        let module = pactl(&[arguments.as_slice(), &["adjust_time=0"]].concat())
            .or_else(|_| pactl(&arguments))?
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

        let mut sink = Self { module, subscribe, default };
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
    /// Cada stream volta à saída padrão antes de o módulo cair. Deixar o descarregamento do
    /// `module-combine-sink` mover sozinho o que estava nele derruba o PulseAudio 16
    /// (`Assertion 'size < (1024*1024*96)' failed` no `pa_xmalloc`, reproduzido no Ubuntu
    /// 24.04): parar de compartilhar com um jogo tocando calava a máquina inteira.
    fn drop(&mut self) {
        let _ = self.subscribe.kill();
        let _ = self.subscribe.wait();

        if let (Ok(sinks), Ok(inputs)) = (pactl(&["list", "sinks", "short"]), pactl(&["list", "sink-inputs", "short"]))
            && let Some(ours) = index_of_sink(&sinks, SINK)
        {
            for input in inputs_on(&inputs, ours) {
                if let Err(error) = pactl(&["move-sink-input", &input.to_string(), &self.default]) {
                    tracing::warn!(%error, input, "captura: o stream não voltou à saída padrão antes de o sink cair");
                }
            }
        }

        let _ = pactl(&["unload-module", &self.module.to_string()]);
    }
}

/// O que um app que morreu com a tela no ar (o `kill -9`, a queda) deixou de pé: o
/// `unkvoid_share` com o jogo dentro. Sem limpar, o `load-module` seguinte esbarrava no nome e o
/// som da tela não subia mais, e o jogo seguia preso no sink órfão. Cada stream volta à saída em
/// que o órfão desaguava antes de o módulo cair, pelo mesmo motivo do `Drop`.
fn clear_leftovers() {
    let Ok(modules) = pactl(&["list", "modules", "short"]) else {
        return;
    };

    for (module, slave) in leftovers(&modules) {
        tracing::warn!(module, "captura: o sink de uma transmissão que morreu ainda estava de pé, e saiu");

        if let (Ok(sinks), Ok(inputs)) = (pactl(&["list", "sinks", "short"]), pactl(&["list", "sink-inputs", "short"]))
            && let Some(orphan) = index_of_sink(&sinks, SINK)
        {
            for input in inputs_on(&inputs, orphan) {
                let _ = pactl(&["move-sink-input", &input.to_string(), &slave]);
            }
        }

        let _ = pactl(&["unload-module", &module.to_string()]);
    }
}

/// Os `module-combine-sink` com o nosso sink, na listagem curta dos módulos (`índice\tnome\t
/// argumentos`), e a saída em que cada um desaguava.
fn leftovers(short: &str) -> Vec<(u32, String)> {
    short
        .lines()
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let index = columns.next()?.trim().parse().ok()?;
            let arguments = (columns.next()? == "module-combine-sink").then(|| columns.next())??;
            let mine = arguments.split_whitespace().any(|argument| argument == format!("sink_name={SINK}"));
            let slave = arguments.split_whitespace().find_map(|argument| argument.strip_prefix("slaves="))?;

            mine.then(|| (index, slave.split(',').next().unwrap_or(slave).to_string()))
        })
        .collect()
}

/// O índice de um sink pelo nome, na listagem curta do `pactl` (`índice\tnome\t...`).
fn index_of_sink(short: &str, name: &str) -> Option<u32> {
    short.lines().find_map(|line| {
        let mut columns = line.split('\t');
        let index = columns.next()?.trim().parse().ok()?;

        (columns.next()? == name).then_some(index)
    })
}

/// Os streams que tocam num sink, na listagem curta do `pactl` (`índice\tsink\t...`).
fn inputs_on(short: &str, sink: u32) -> Vec<u32> {
    short
        .lines()
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let index = columns.next()?.trim().parse().ok()?;

            (columns.next()?.trim().parse::<u32>().ok()? == sink).then_some(index)
        })
        .collect()
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
    fn the_streams_on_our_sink_are_found_by_its_index() {
        let sinks = "1\tfake\tmodule-null-sink.c\ts16le 2ch 44100Hz\tIDLE\n2\tunkvoid_share\tmodule-combine-sink.c\ts16le 2ch 48000Hz\tRUNNING\n";
        let inputs = "3\t2\t11\tprotocol-native.c\ts16le 2ch 48000Hz\n4\t1\t12\tprotocol-native.c\ts16le 2ch 48000Hz\n5\t2\t13\tprotocol-native.c\ts16le 2ch 48000Hz\n";

        assert_eq!(index_of_sink(sinks, SINK), Some(2));
        assert_eq!(index_of_sink(sinks, "outro"), None);
        assert_eq!(inputs_on(inputs, 2), vec![3, 5]);
    }

    #[test]
    fn a_leftover_sink_is_found_with_the_output_it_fed() {
        let modules = "7\tmodule-native-protocol-unix\t\t\n\
                       23\tmodule-combine-sink\tsink_name=unkvoid_share slaves=alsa_output.pci adjust_time=0 sink_properties=device.description=Unkvoid\t\n\
                       24\tmodule-combine-sink\tsink_name=outro slaves=fake\t\n";

        assert_eq!(leftovers(modules), vec![(23, "alsa_output.pci".to_string())]);
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

        // Soltos pelo `sh`, e não filhos nossos: filho nosso é `gst-launch`, e fica de fora
        // por regra. O jogo de verdade também não é filho do app.
        for name in ["Discord", "cs2"] {
            let path = scratch.join(name);
            std::fs::write(&path, &paplay).unwrap();
            std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
            Command::new("sh")
                .arg("-c")
                .arg(format!("{} --volume=0 --raw /dev/zero </dev/null >/dev/null 2>&1 &", path.display()))
                .status()
                .unwrap();
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

        let _ = Command::new("pkill").args(["-f", "unkvoid-audio-test"]).status();
    }

    /// Parar de compartilhar com um jogo tocando dentro do nosso sink, várias vezes seguidas: o
    /// servidor de som continua de pé, e o jogo volta à saída padrão. No PulseAudio 16, deixar
    /// o descarregamento do `module-combine-sink` mover o stream sozinho derrubava o servidor
    /// (`Assertion 'size < (1024*1024*96)' failed` no `pa_xmalloc`) — e o som da máquina toda.
    #[test]
    #[ignore]
    fn stopping_the_share_with_a_game_playing_keeps_the_sound_server_alive() {
        let default = pactl(&["get-default-sink"]).expect("pulseaudio no ar").trim().to_string();
        let which = Command::new("which").arg("paplay").output().unwrap().stdout;
        let paplay = std::fs::read(String::from_utf8(which).unwrap().trim()).unwrap();
        let scratch = std::env::temp_dir().join("unkvoid-audio-stop");
        let game = scratch.join("jogo");

        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(&game, &paplay).unwrap();
        std::fs::set_permissions(&game, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        // Volume zero: o que derruba o PulseAudio é a taxa e o descarregamento, não o som, e um
        // chiado em volume cheio na saída de quem roda o teste não prova nada.
        Command::new("sh").arg("-c").arg(format!("{} --volume=0 --raw /dev/urandom </dev/null >/dev/null 2>&1 &", game.display())).status().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));

        for round in 1..=6 {
            let sink = SharedSink::open(false).expect("o combine sink sobe");

            std::thread::sleep(std::time::Duration::from_millis(400));
            drop(sink);
            std::thread::sleep(std::time::Duration::from_millis(400));

            assert!(pactl(&["info"]).is_ok(), "o servidor de som caiu ao parar de compartilhar, na rodada {round}");
        }

        let listing = pactl(&["list", "sink-inputs"]).unwrap();
        let sinks = pactl(&["list", "sinks", "short"]).unwrap();
        let game_sink = listing
            .split("Sink Input #")
            .find(|block| block.contains("application.process.binary = \"jogo\""))
            .and_then(|block| block.lines().find_map(|line| line.trim().strip_prefix("Sink: ")).map(|index| index.trim().to_string()))
            .expect("o jogo ainda toca");

        assert!(sinks.lines().any(|line| line.starts_with(&format!("{game_sink}\t{default}\t"))), "o jogo não voltou à saída padrão");

        let _ = Command::new("pkill").args(["-f", "unkvoid-audio-stop"]).status();
    }

    /// O sink em que o jogo toca agora, pelo nome.
    fn sink_of(binary: &str) -> Option<String> {
        let listing = pactl(&["list", "sink-inputs"]).ok()?;
        let sinks = pactl(&["list", "sinks", "short"]).ok()?;
        let index = listing
            .split("Sink Input #")
            .find(|block| block.contains(&format!("application.process.binary = \"{binary}\"")))?
            .lines()
            .find_map(|line| line.trim().strip_prefix("Sink: "))?
            .trim()
            .to_string();

        sinks.lines().find_map(|line| {
            let mut columns = line.split('\t');

            (columns.next()? == index).then(|| columns.next().map(str::to_string))?
        })
    }

    /// O app morreu com a tela no ar (o `kill -9`): o `unkvoid_share` ficou carregado com o jogo
    /// dentro, e a transmissão seguinte não conseguia subir o sink (o nome já existia). O `open`
    /// seguinte limpa o órfão, e no fim o jogo volta à saída padrão (conferência do Tux no #53).
    #[test]
    #[ignore]
    fn a_sink_left_by_a_dead_app_is_cleared_by_the_next_share() {
        let default = pactl(&["get-default-sink"]).expect("pulseaudio no ar").trim().to_string();
        let which = Command::new("which").arg("paplay").output().unwrap().stdout;
        let paplay = std::fs::read(String::from_utf8(which).unwrap().trim()).unwrap();
        let scratch = std::env::temp_dir().join("unkvoid-audio-orphan");
        let game = scratch.join("jogo-orfao");
        let wait = || std::thread::sleep(std::time::Duration::from_millis(500));

        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(&game, &paplay).unwrap();
        std::fs::set_permissions(&game, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        Command::new("sh").arg("-c").arg(format!("{} --volume=0 --raw /dev/zero </dev/null >/dev/null 2>&1 &", game.display())).status().unwrap();
        wait();

        // A queda: nada do `Drop` roda, só o processo do `pactl subscribe` morre junto.
        let mut crashed = std::mem::ManuallyDrop::new(SharedSink::open(false).expect("o sink sobe"));

        let _ = crashed.subscribe.kill();
        let _ = crashed.subscribe.wait();
        wait();

        assert_eq!(sink_of("jogo-orfao").as_deref(), Some(SINK), "o jogo entrou no sink da transmissão");

        let sink = SharedSink::open(false).expect("a transmissão seguinte sobe o sink");

        wait();

        let ours = pactl(&["list", "sinks", "short"]).unwrap().lines().filter(|line| line.split('\t').nth(1).is_some_and(|name| name.starts_with(SINK))).count();

        assert_eq!(ours, 1, "o sink órfão continuou de pé ao lado do novo");
        assert_eq!(sink_of("jogo-orfao").as_deref(), Some(SINK), "o jogo sobe na transmissão nova");

        drop(sink);
        wait();

        assert!(leftovers(&pactl(&["list", "modules", "short"]).unwrap()).is_empty(), "sobrou módulo");
        assert_eq!(sink_of("jogo-orfao"), Some(default), "o jogo não voltou à saída padrão");
        assert!(pactl(&["info"]).is_ok(), "o servidor de som caiu");

        let _ = Command::new("pkill").args(["-f", "unkvoid-audio-orphan"]).status();
    }
}
