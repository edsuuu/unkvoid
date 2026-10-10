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
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};

use crate::CaptureConfig;

/// O começo do nome de cada sink: `unkvoid_share_<pid>_<n>`. O pid diz à limpeza se o dono ainda
/// vive, e outra instância do app na mesma máquina não tem o sink derrubado. Até a 0.1.17 o nome
/// era só este, sem dono.
const SINK: &str = "unkvoid_share";

/// Os sinks que este processo tem de pé. Um com o pid dele que não está aqui é de um processo
/// morto que teve o mesmo pid.
static OURS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static NEXT: AtomicU32 = AtomicU32::new(0);

/// Os daemons de som: os streams internos deles (o próprio combine, loopbacks) nunca
/// entram no nosso sink, senão o som dá a volta e realimenta.
const DAEMONS: &[&str] = &["pulseaudio", "pipewire", "pipewire-pulse", "wireplumber"];

/// O sink de pé: descarregá-lo é o `Drop`.
pub struct SharedSink {
    module: u32,
    name: String,
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

        let name = format!("{SINK}_{}_{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        let (sink_name, slaves) = (format!("sink_name={name}"), format!("slaves={default}"));
        let arguments = ["load-module", "module-combine-sink", &sink_name, &slaves, "sink_properties=device.description=Unkvoid"];
        let module: u32 = pactl(&[arguments.as_slice(), &["adjust_time=0"]].concat())
            .or_else(|_| pactl(&arguments))?
            .trim()
            .parse()
            .map_err(|_| "o pactl não devolveu o índice do módulo".to_string())?;

        ours().push(name.clone());

        let subscribe = match Command::new("pactl").env("LC_ALL", "C").arg("subscribe").stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
            Ok(subscribe) => subscribe,
            Err(error) => {
                let _ = pactl(&["unload-module", &module.to_string()]);
                ours().retain(|sink| *sink != name);

                return Err(format!("pactl subscribe não abriu ({error})"));
            }
        };

        let mut sink = Self { module, name: name.clone(), subscribe, default };
        let muted: Vec<String> = if mute_listed_apps {
            CaptureConfig::MUTED_EXECUTABLES.iter().map(|name| name.trim_end_matches(".exe").to_ascii_lowercase()).collect()
        } else {
            Vec::new()
        };

        route(module, &name, &muted);

        if let Some(events) = sink.subscribe.stdout.take() {
            std::thread::spawn(move || {
                for line in BufReader::new(events).lines().map_while(Result::ok) {
                    if line.contains("'new' on sink-input") {
                        route(module, &name, &muted);
                    }
                }
            });
        }

        Ok(sink)
    }

    /// O device que o `pulsesrc` lê enquanto o sink está de pé.
    pub fn monitor(&self) -> String {
        format!("{}.monitor", self.name)
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
            && let Some(ours) = index_of_sink(&sinks, &self.name)
        {
            for input in inputs_on(&inputs, ours) {
                if let Err(error) = pactl(&["move-sink-input", &input.to_string(), &self.default]) {
                    tracing::warn!(%error, input, "captura: o stream não voltou à saída padrão antes de o sink cair");
                }
            }
        }

        let _ = pactl(&["unload-module", &self.module.to_string()]);

        ours().retain(|sink| *sink != self.name);
    }
}

fn ours() -> std::sync::MutexGuard<'static, Vec<String>> {
    OURS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// O que um app que morreu com a tela no ar (o `kill -9`, a queda) deixou de pé: o sink dele com
/// o jogo dentro, que ficava na lista de saídas e com o jogo preso. Cada stream volta à saída em
/// que o órfão desaguava antes de o módulo cair, pelo mesmo motivo do `Drop`. O sink de outra
/// instância viva do app fica: derrubá-lo cortava o som da tela dela no meio.
fn clear_leftovers() {
    let Ok(modules) = pactl(&["list", "modules", "short"]) else {
        return;
    };

    for leftover in leftovers(&modules, abandoned) {
        tracing::warn!(module = leftover.module, sink = %leftover.sink, "captura: o sink de uma transmissão que morreu ainda estava de pé, e saiu");

        if let (Ok(sinks), Ok(inputs)) = (pactl(&["list", "sinks", "short"]), pactl(&["list", "sink-inputs", "short"]))
            && let Some(orphan) = index_of_sink(&sinks, &leftover.sink)
        {
            for input in inputs_on(&inputs, orphan) {
                let _ = pactl(&["move-sink-input", &input.to_string(), &leftover.slave]);
            }
        }

        let _ = pactl(&["unload-module", &leftover.module.to_string()]);
    }
}

/// Um sink nosso que ninguém vivo segura.
#[derive(Debug, PartialEq, Eq)]
struct Leftover {
    module: u32,
    sink: String,
    /// A saída em que ele desaguava.
    slave: String,
}

/// Os `module-combine-sink` com um sink nosso que `abandoned` diz sem dono, na listagem curta dos
/// módulos (`índice\tnome\targumentos`).
fn leftovers(short: &str, abandoned: impl Fn(&str) -> bool) -> Vec<Leftover> {
    short
        .lines()
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let module = columns.next()?.trim().parse().ok()?;
            let arguments = (columns.next()? == "module-combine-sink").then(|| columns.next())??;
            let sink = arguments.split_whitespace().find_map(|argument| argument.strip_prefix("sink_name="))?;
            let slave = arguments.split_whitespace().find_map(|argument| argument.strip_prefix("slaves="))?;
            let ours = sink == SINK || sink.strip_prefix(SINK).is_some_and(|rest| rest.starts_with('_'));

            (ours && abandoned(sink)).then(|| Leftover { module, sink: sink.to_string(), slave: slave.split(',').next().unwrap_or(slave).to_string() })
        })
        .collect()
}

/// Se o dono do sink morreu: o pid do nome não existe mais, ou é o deste processo e o sink não é
/// um dos que ele tem de pé. O nome sem pid (até a 0.1.17) conta como morto: não há como saber de
/// quem é, e o app novo substitui o velho na atualização.
fn abandoned(sink: &str) -> bool {
    let Some(pid) = owner_of(sink) else {
        return true;
    };

    if pid == std::process::id() {
        return !ours().iter().any(|held| held == sink);
    }

    !std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// O pid no nome do sink (`unkvoid_share_<pid>_<n>`).
fn owner_of(sink: &str) -> Option<u32> {
    sink.strip_prefix(SINK)?.strip_prefix('_')?.split('_').next()?.parse().ok()
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
fn route(module: u32, sink: &str, muted: &[String]) {
    let Ok(listing) = pactl(&["list", "sink-inputs"]) else {
        return;
    };

    for index in targets(&listing, module, muted, std::process::id(), parent_of) {
        if let Err(error) = pactl(&["move-sink-input", &index.to_string(), sink]) {
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
        let sinks = "1\tfake\tmodule-null-sink.c\ts16le 2ch 44100Hz\tIDLE\n2\tunkvoid_share_4242_0\tmodule-combine-sink.c\ts16le 2ch 48000Hz\tRUNNING\n";
        let inputs = "3\t2\t11\tprotocol-native.c\ts16le 2ch 48000Hz\n4\t1\t12\tprotocol-native.c\ts16le 2ch 48000Hz\n5\t2\t13\tprotocol-native.c\ts16le 2ch 48000Hz\n";

        assert_eq!(index_of_sink(sinks, "unkvoid_share_4242_0"), Some(2));
        assert_eq!(index_of_sink(sinks, "outro"), None);
        assert_eq!(inputs_on(inputs, 2), vec![3, 5]);
    }

    /// O sink de um processo morto e o do nome antigo saem com a saída em que desaguavam; o de um
    /// vivo fica, e o de outro programa nem entra na conta.
    #[test]
    fn only_the_sinks_whose_owner_died_are_leftovers() {
        let modules = "7\tmodule-native-protocol-unix\t\t\n\
                       23\tmodule-combine-sink\tsink_name=unkvoid_share slaves=alsa_output.pci adjust_time=0 sink_properties=device.description=Unkvoid\t\n\
                       24\tmodule-combine-sink\tsink_name=outro slaves=fake\t\n\
                       25\tmodule-combine-sink\tsink_name=unkvoid_share_100_0 slaves=fone,alto adjust_time=0\t\n\
                       26\tmodule-combine-sink\tsink_name=unkvoid_share_200_3 slaves=fone adjust_time=0\t\n\
                       27\tmodule-combine-sink\tsink_name=unkvoid_sharing slaves=fone\t\n";
        let dead = |sink: &str| owner_of(sink).is_none_or(|pid| pid == 100);

        assert_eq!(
            leftovers(modules, dead),
            vec![
                Leftover { module: 23, sink: "unkvoid_share".into(), slave: "alsa_output.pci".into() },
                Leftover { module: 25, sink: "unkvoid_share_100_0".into(), slave: "fone".into() },
            ]
        );
    }

    #[test]
    fn the_owner_is_the_pid_in_the_name() {
        assert_eq!(owner_of("unkvoid_share_4242_0"), Some(4242));
        assert_eq!(owner_of("unkvoid_share_4242_17"), Some(4242));
        assert_eq!(owner_of("unkvoid_share"), None);
        assert_eq!(owner_of("unkvoid_share_x_0"), None);
        assert!(abandoned("unkvoid_share"), "o nome sem dono é de uma versão velha");
        assert!(abandoned(&format!("unkvoid_share_{}_999", std::process::id())), "com o nosso pid e fora da lista, é de um processo morto com o mesmo pid");
        assert!(!abandoned("unkvoid_share_1_0"), "o pid 1 está vivo");
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

        assert!(sink_of("cs2").ends_with(&sink.name), "jogo em {}", sink_of("cs2"));
        assert!(sink_of("Discord").ends_with(&default), "chamada em {}", sink_of("Discord"));

        let name = sink.name.clone();

        drop(sink);

        std::thread::sleep(std::time::Duration::from_millis(300));

        assert!(!pactl(&["list", "sinks", "short"]).unwrap().contains(&name), "o sink sumiu no fim");

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

    /// Os sinks nossos de pé, pelo nome.
    fn our_sinks() -> Vec<String> {
        pactl(&["list", "sinks", "short"])
            .unwrap()
            .lines()
            .filter_map(|line| line.split('\t').nth(1))
            .filter(|name| name.starts_with(SINK))
            .map(str::to_string)
            .collect()
    }

    /// Sobe um sink como o app subiria, com o nome dado e sem ninguém para descarregá-lo: o que
    /// fica de um app que morreu (o `kill -9`), ou o de outra instância viva.
    fn load_sink(name: &str, slave: &str) -> u32 {
        pactl(&["load-module", "module-combine-sink", &format!("sink_name={name}"), &format!("slaves={slave}"), "adjust_time=0"]).expect("o sink sobe").trim().parse().unwrap()
    }

    /// Um pid que acabou de morrer.
    fn dead_pid() -> u32 {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();

        child.wait().unwrap();

        pid
    }

    /// O app morreu com a tela no ar (o `kill -9`): o sink dele ficou carregado com o jogo
    /// dentro, e o da versão de antes do pid no nome também. O `open` seguinte limpa os dois, e no
    /// fim o jogo volta à saída padrão (conferência do Tux no #53).
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

        let orphan = format!("{SINK}_{}_0", dead_pid());

        load_sink(SINK, &default);
        load_sink(&orphan, &default);

        let game_input = pactl(&["list", "sink-inputs"]).unwrap().split("Sink Input #").find(|block| block.contains("application.process.binary = \"jogo-orfao\"")).and_then(|block| block.lines().next()).map(|line| line.trim().to_string()).expect("o jogo toca");

        pactl(&["move-sink-input", &game_input, &orphan]).expect("o jogo entra no sink do app morto");
        wait();

        assert_eq!(sink_of("jogo-orfao").as_deref(), Some(orphan.as_str()), "o jogo entrou no sink do app morto");

        let sink = SharedSink::open(false).expect("a transmissão seguinte sobe o sink");

        wait();

        assert_eq!(our_sinks(), vec![sink.name.clone()], "um órfão continuou de pé ao lado do novo");
        assert_eq!(sink_of("jogo-orfao").as_deref(), Some(sink.name.as_str()), "o jogo sobe na transmissão nova");

        drop(sink);
        wait();

        assert!(our_sinks().is_empty(), "sobrou sink: {:?}", our_sinks());
        assert_eq!(sink_of("jogo-orfao"), Some(default), "o jogo não voltou à saída padrão");
        assert!(pactl(&["info"]).is_ok(), "o servidor de som caiu");

        let _ = Command::new("pkill").args(["-f", "unkvoid-audio-orphan"]).status();
    }

    /// Duas instâncias do app na mesma máquina, e duas transmissões no mesmo processo: subir um
    /// sink nunca derruba o de quem está vivo. O da instância que morre sai na transmissão
    /// seguinte (achado do Warden no #53).
    #[test]
    #[ignore]
    fn the_sink_of_a_live_instance_is_never_cleared() {
        let default = pactl(&["get-default-sink"]).expect("pulseaudio no ar").trim().to_string();
        let mut other = Command::new("sleep").arg("30").spawn().unwrap();
        let theirs = format!("{SINK}_{}_0", other.id());
        let module = load_sink(&theirs, &default);

        let first = SharedSink::open(false).expect("o primeiro sink sobe");
        let second = SharedSink::open(false).expect("o segundo sobe ao lado");
        let mut expected = vec![theirs.clone(), first.name.clone(), second.name.clone()];

        expected.sort();

        let mut standing = our_sinks();

        standing.sort();

        assert_eq!(standing, expected, "um sink vivo caiu");

        drop((first, second));

        assert_eq!(our_sinks(), vec![theirs.clone()], "parar de compartilhar derrubou o da outra instância");

        other.kill().unwrap();
        other.wait().unwrap();

        let after = SharedSink::open(false).expect("a transmissão seguinte sobe");

        assert_eq!(our_sinks(), vec![after.name.clone()], "o sink da instância morta ficou");

        drop(after);

        let _ = pactl(&["unload-module", &module.to_string()]);

        assert!(our_sinks().is_empty(), "sobrou sink: {:?}", our_sinks());
        assert!(pactl(&["info"]).is_ok(), "o servidor de som caiu");
    }
}
