import Echo from 'laravel-echo';
import Pusher from 'pusher-js';

import { ApiClient } from './ApiClient.js';
import { Chat } from './Chat.js';
import { Clips } from './Clips.js';
import { Permissions } from './Permissions.js';
import { ServerSettings } from './ServerSettings.js';
import { Voice } from './Voice.js';

const el = id => document.getElementById(id);

const { invoke } = window.__TAURI__.core;

/**
 * O modo servidor: trilho | canais | centro | membros, para quem entrou com conta.
 *
 * Tudo o que aparece vem da árvore do `GET /api/servers/{id}`, e ela é refeita a cada
 * `ServerUpdated`. O app só esconde botão: cada 403 do Laravel vira aviso no canto.
 */
export class Hub {
    /** Quanto esperar depois do último `ServerUpdated` antes de refazer a árvore. */
    static REFRESH_DEBOUNCE_MS = 250;

    constructor(app, server) {
        this.app = app;
        this.server = server;
        this.api = new ApiClient(server);
        this.user = null;
        this.config = null;
        this.echo = null;
        this.servers = [];
        this.tree = null;
        this.channel = null;
        this.online = new Set();
        this.chat = new Chat(this);
        this.voice = new Voice(app, this);
        this.settings = new ServerSettings(this);
        this.clips = new Clips(app, this);
        this.menuTarget = null;

        /** Sobe a cada `openServer`: uma árvore que chegou atrasada não pisa na nova. */
        this.openTicket = 0;
        this.refreshTimer = null;

        /** Os canais de voz cujo canal privado está assinado (`VoiceStateUpdated`). */
        this.voiceChannels = new Set();

        this.wireHub();
    }

    /** Roda uma chamada à API e transforma a recusa (403, 422…) em aviso. */
    async attempt(work) {
        try {
            return await work();
        } catch (failure) {
            this.app.log('hub.error', { status: failure.status ?? null, message: failure.message ?? String(failure) });

            if (failure.status === 401) {
                this.app.toast('sua sessão expirou, entre de novo', true);
                await this.logout();

                return undefined;
            }

            this.app.toast(failure.status === 403 ? `sem permissão: ${failure.message}` : failure.message ?? String(failure), true);

            return undefined;
        }
    }

    /**
     * O nome do evento nas duas grafias: com o ponto na frente o Echo usa o nome cru
     * (`broadcastAs`), sem ele prefixa `App\Events\`. O Laravel do outro lado escolhe.
     */
    listen(subscription, name, handler) {
        subscription.listen(`.${name}`, handler).listen(name, handler);
    }

    async restore() {
        if (! this.api.token) {
            return false;
        }

        try {
            this.user = await this.api.get('/api/me');
        } catch (failure) {
            this.app.log('hub.restore.error', { status: failure.status ?? null, message: failure.message ?? String(failure) });

            if (failure.status === 401) {
                this.api.setToken(null);
            }

            return false;
        }

        await this.open();

        return true;
    }

    wireEntry() {
        const finish = async ({ token, user }) => {
            this.api.setToken(token);
            this.user = user ?? await this.api.get('/api/me');
            await this.open();
        };
        const tryLogin = work => async event => {
            event.preventDefault();
            el('login-error').textContent = '';

            try {
                await finish(await work());
            } catch (failure) {
                this.app.log('hub.login.error', { message: failure.message ?? String(failure) });
                el('login-error').textContent = failure.message ?? String(failure);
            }
        };

        el('back-to-hub').hidden = ! this.user;
        el('back-to-hub').onclick = () => void this.open();
        el('login-form').onsubmit = tryLogin(() => this.api.post('/api/auth/login', {
            email: el('login-email').value.trim(),
            password: el('login-password').value,
            device: 'app',
        }));
        el('register-form').onsubmit = tryLogin(() => this.api.post('/api/auth/register', {
            name: el('register-name').value.trim(),
            email: el('register-email').value.trim(),
            password: el('register-password').value,
            device: 'app',
        }));
        el('register-toggle').onclick = () => {
            const registering = el('register-form').hidden;

            el('register-form').hidden = ! registering;
            el('login-form').hidden = registering;
            el('register-toggle').textContent = registering ? 'Já tenho conta' : 'Criar conta';
        };

        // O Rust abre uma porta em 127.0.0.1, manda o navegador para o site com ela, e o
        // callback do Google devolve o token nessa porta. Se o navegador não abrir (ou a
        // pessoa fechar a aba), sobra o campo para colar o token à mão.
        el('login-google').onclick = tryLogin(async () => {
            el('google-token-block').hidden = false;
            el('login-error').textContent = 'Termine o login no navegador que abriu…';

            try {
                return { token: await invoke('google_login', { server: this.server }) };
            } catch (failure) {
                el('google-token').placeholder = 'Cole o token (ou a URL inteira) que o navegador mostrou';
                throw new Error(`o navegador não devolveu o token (${failure.message ?? failure}); cole-o aqui`);
            }
        });
        el('google-token-use').onclick = tryLogin(async () => {
            const pasted = el('google-token').value.trim();
            const token = pasted.includes('token=') ? new URL(pasted).searchParams.get('token') : pasted;

            if (! token) {
                throw new Error('cole o token que o navegador mostrou');
            }

            return { token };
        });
    }

    async open() {
        el('entry-screen').hidden = true;
        el('room').hidden = true;
        el('hub').hidden = false;
        document.body.dataset.account = 'on';
        this.clips.refresh();
        el('me-name').textContent = this.user.name;
        el('me-avatar').textContent = this.user.name.slice(0, 1).toUpperCase();

        this.config ??= await this.attempt(() => this.api.get('/api/config'));

        if (! this.config) {
            return;
        }

        // Voltar da sala por código não reconecta: o Echo da sessão continua vivo.
        if (! this.echo) {
            this.connectEcho();
        }

        await this.attempt(() => this.loadServers());
    }

    connectEcho() {
        const { host, port, key, scheme } = this.config.reverb;

        window.Pusher = Pusher;
        this.echo = new Echo({
            broadcaster: 'reverb',
            Pusher,
            key,
            wsHost: host,
            wsPort: port,
            wssPort: port,
            forceTLS: scheme === 'https',
            enabledTransports: ['ws', 'wss'],
            authEndpoint: `${this.server}/broadcasting/auth`,
            auth: { headers: { Authorization: `Bearer ${this.api.token}` } },
        });

        // Sem isto uma queda do Reverb era chat parado sem uma palavra: as mensagens
        // dos outros simplesmente deixavam de chegar.
        this.echo.connector.pusher.connection.bind('state_change', ({ current }) => {
            el('me-connection').hidden = current === 'connected';
        });

        const own = this.echo.private(`user.${this.user.id}`);

        // No canal da conta, e não no do servidor: o clipe chega mesmo sem servidor aberto.
        this.listen(own, 'ClipUpdated', ({ clip }) => this.clips.update(clip));
        this.listen(own, 'MemberRemoved', ({ server_id: serverId, reason }) => {
            this.app.toast(reason === 'banned' ? 'você foi banido deste servidor' : 'você foi expulso deste servidor', true);

            if (this.tree?.id === serverId) {
                void this.closeServer();
            }

            void this.loadServers();
        });
    }

    async logout() {
        await this.voice.leave();
        await this.closeServer();
        this.echo?.disconnect();
        this.echo = null;
        this.clips.forget();
        this.api.setToken(null);
        this.user = null;
        delete document.body.dataset.account;
        this.app.showEntry();
    }

    wireHub() {
        el('logout').onclick = () => void this.logout();
        // A sala anônima usa o mesmo palco e o mesmo cliente de mídia: a voz sai antes.
        el('room-by-code').onclick = () => void this.voice.leave().then(() => {
            this.clips.showTab('broadcast');
            this.app.showEntry();
        });
        el('server-add').onclick = () => {
            el('server-modal').hidden = false;
            el('server-create-name').focus();
        };
        el('server-modal-close').onclick = () => { el('server-modal').hidden = true; };
        el('server-create-form').onsubmit = event => {
            event.preventDefault();
            void this.attempt(async () => {
                el('server-modal').hidden = true;
                await this.createServer(el('server-create-name').value.trim());
                el('server-create-name').value = '';
            });
        };
        el('home-create-form').onsubmit = event => {
            event.preventDefault();
            void this.attempt(async () => {
                await this.createServer(el('home-create-name').value.trim());
                el('home-create-name').value = '';
            });
        };
        el('invite-banner-close').onclick = () => { el('invite-banner').hidden = true; };
        el('invite-banner-copy').onclick = () => void navigator.clipboard.writeText(this.tree?.invite_code ?? '')
            .then(() => this.app.toast('convite copiado'))
            .catch(failure => this.app.log('hub.invite.copy.error', { message: failure.message ?? String(failure) }));
        el('server-join-form').onsubmit = event => {
            event.preventDefault();
            void this.attempt(async () => {
                const joined = await this.api.post(`/api/invites/${el('server-join-code').value.trim()}`);

                el('server-modal').hidden = true;
                el('server-join-code').value = '';
                await this.loadServers(joined.id);
            });
        };
        el('server-settings').onclick = () => this.settings.open();
        el('channel-add').onclick = () => this.settings.openChannelModal(null);
        el('channel-edit').onclick = () => this.settings.openChannelModal(this.channel);
        this.wireMemberMenu();
    }

    /**
     * Não abre o primeiro servidor sozinho: logado e sem servidor aberto, o centro é a casa
     * (Criar sala e Últimas salas). Só abre o pedido ou o que já estava aberto.
     */
    async loadServers(openId = null) {
        this.servers = await this.api.get('/api/servers');
        this.drawRail();
        this.drawHome();

        const target = openId ?? this.tree?.id;

        if (target && this.servers.some(server => server.id === target)) {
            await this.openServer(target);

            return;
        }

        await this.closeServer();
    }

    /** Na ordem do `GET /api/servers`: o Laravel já ordena pelo último acesso à voz. */
    drawHome() {
        el('home-servers').replaceChildren(...this.servers.map(server => {
            const button = document.createElement('button');

            button.type = 'button';
            button.className = 'row-item w-full cursor-pointer text-left';
            button.innerHTML = '<span class="min-w-0 flex-1 truncate text-[13.5px] font-semibold"></span><span class="label-mono"></span>';
            button.querySelector('span').textContent = server.name;
            button.querySelectorAll('span')[1].textContent = server.last_accessed_at
                ? new Date(server.last_accessed_at).toLocaleDateString('pt-BR')
                : 'nunca na voz';
            button.onclick = () => void this.attempt(() => this.openServer(server.id));

            return button;
        }));
        el('home-servers-empty').hidden = this.servers.length > 0;
    }

    /** Cria e abre sem entrar na voz, e já mostra o convite: mandá-lo é o próximo passo. */
    async createServer(name) {
        const created = await this.api.post('/api/servers', { name });

        await this.loadServers(created.id);
        el('invite-banner-code').textContent = this.tree?.invite_code ?? '';
        el('invite-banner').hidden = ! this.tree?.invite_code;
    }

    drawRail() {
        const list = el('server-list');

        list.innerHTML = '';

        for (const server of this.servers) {
            const button = document.createElement('button');
            const active = server.id === this.tree?.id;

            button.type = 'button';
            button.title = server.name;
            button.className = `row-item w-full cursor-pointer text-left ${active ? 'row-item-on' : ''}`;
            button.innerHTML = `<span class="avatar size-[30px] shrink-0 rounded-[10px] text-[11px] ${active ? '' : 'avatar-flat'}"></span>`
                + '<span class="min-w-0"><span class="block truncate text-[13.5px] font-semibold"></span>'
                + '<span class="label-mono mt-0.5 block"></span></span>';

            const [initials, , name, role] = button.querySelectorAll('span');

            initials.textContent = server.name.split(/\s+/).slice(0, 2).map(word => word[0] ?? '').join('').toUpperCase();
            name.textContent = server.name;
            role.textContent = (active ? 'atual · ' : '') + (server.owner_id === this.user.id ? 'dono' : 'membro');
            button.onclick = () => void this.attempt(() => this.openServer(server.id));
            list.appendChild(button);
        }
    }

    async openServer(id) {
        const switching = this.tree?.id !== id;

        // O `closeServer` também sobe o contador: tirar o bilhete antes dele fazia toda
        // troca de servidor descartar a própria árvore como se tivesse chegado atrasada.
        if (switching) {
            await this.closeServer();
        }

        const ticket = ++this.openTicket;
        const tree = await this.api.get(`/api/servers/${id}`);

        // Outro `openServer` passou na frente enquanto esta árvore vinha.
        if (ticket !== this.openTicket) {
            return;
        }

        this.tree = tree;

        if (switching) {
            this.joinPresence();
        }

        this.subscribeVoiceStates();

        // A voz e o canal aberto podem ter mudado de permissão (ou sumido) na árvore nova.
        if (this.voice.channel) {
            this.voice.channel = this.tree.channels.find(channel => channel.id === this.voice.channel.id) ?? this.voice.channel;
        }

        this.drawRail();
        this.drawServer();
        this.syncVoiceSources();

        const current = this.tree.channels.find(channel => channel.id === this.channel?.id);

        if (current) {
            this.channel = current;
            this.drawCenter();
        } else if (this.channel || switching) {
            await this.openChannel(this.tree.channels.find(channel => channel.type === 'text') ?? null);
        }
    }

    async closeServer() {
        this.openTicket += 1;
        clearTimeout(this.refreshTimer);

        if (this.tree) {
            this.echo?.leave(`server.${this.tree.id}`);
        }

        for (const channelId of this.voiceChannels) {
            this.echo?.leave(`channel.${channelId}`);
        }

        this.voiceChannels.clear();

        // Sair do servidor não derruba a voz de outro canal: só se ela era deste.
        if (this.voice.channel && this.tree?.channels.some(channel => channel.id === this.voice.channel.id)) {
            await this.voice.leave();
        }

        this.chat.close();
        this.tree = null;
        this.channel = null;
        this.online.clear();
        el('invite-banner').hidden = true;
        this.drawServer();
        this.drawCenter();
    }

    joinPresence() {
        const presence = this.echo.join(`server.${this.tree.id}`);

        presence
            .here(users => {
                this.online = new Set(users.map(user => user.id));
                this.drawMembers();
            })
            .joining(user => {
                this.online.add(user.id);
                this.paintPresence(user.id);
            })
            .leaving(user => {
                this.online.delete(user.id);
                this.paintPresence(user.id);
            });

        // Uma rajada de mudanças (cargo, canal, cargo…) vira UMA busca da árvore.
        this.listen(presence, 'ServerUpdated', () => {
            clearTimeout(this.refreshTimer);
            this.refreshTimer = setTimeout(() => {
                if (this.tree) {
                    void this.attempt(() => this.openServer(this.tree.id));
                }
            }, Hub.REFRESH_DEBOUNCE_MS);
        });
    }

    /**
     * `VoiceStateUpdated` chega pelo canal privado de cada canal de voz, e não pela
     * presença do servidor: assim quem não vê o canal não fica sabendo quem entra nele.
     * Assina os que a árvore mostra e larga os que sumiram dela.
     */
    subscribeVoiceStates() {
        const visible = new Set(this.tree.channels.filter(channel => channel.type === 'voice').map(channel => channel.id));

        for (const channelId of this.voiceChannels) {
            if (! visible.has(channelId)) {
                this.echo.leave(`channel.${channelId}`);
                this.voiceChannels.delete(channelId);
            }
        }

        for (const channelId of visible) {
            if (this.voiceChannels.has(channelId)) {
                continue;
            }

            this.voiceChannels.add(channelId);
            this.listen(this.echo.private(`channel.${channelId}`), 'VoiceStateUpdated', ({ channel_id: id, user_id: userId, name, event }) => {
                if (! this.tree?.channels.some(channel => channel.id === id)) {
                    return;
                }

                const people = (this.tree.voice[id] ?? []).filter(person => person.user_id !== userId);

                this.tree.voice[id] = event === 'joined' ? [...people, { user_id: userId, name, sources: [] }] : people;
                this.drawChannels();
            });
        }
    }

    /** O que cada pessoa do canal de voz atual publica, pelo retrato do SFU. */
    syncVoiceSources() {
        const channel = this.voice.channel;
        const peers = this.app.sfu?.peers;

        if (! this.tree || ! channel || ! peers) {
            return;
        }

        const byUser = new Map([...peers.values()].map(peer => [peer.self ? `user:${this.user.id}` : peer.userId, peer]));

        for (const person of this.tree.voice[channel.id] ?? []) {
            const peer = byUser.get(`user:${person.user_id}`);

            if (peer) {
                person.sources = peer.producers.map(producer => producer.source);
                person.muted = peer.producers.some(producer => producer.source === 'mic' && producer.paused);
            }
        }

        this.drawChannels();
        this.voice.paintClip();
    }

    me() {
        return this.tree?.members.find(member => member.user_id === this.user.id) ?? null;
    }

    can(flag) {
        return Boolean(this.tree) && Permissions.has(this.tree.me.permissions, flag);
    }

    drawServer() {
        el('server-name').textContent = this.tree?.name ?? 'Nenhum servidor';
        el('server-settings').hidden = ! this.tree;
        el('channel-add').hidden = ! this.can(Permissions.MANAGE_CHANNELS);
        this.drawChannels();
        this.drawMembers();
    }

    drawChannels() {
        const list = el('channel-list');

        list.innerHTML = '';

        if (! this.tree) {
            list.innerHTML = '<p class="px-2 text-sm text-ink-dim">Crie um servidor ou entre com um convite no “+”.</p>';

            return;
        }

        const channels = [...this.tree.channels].sort((left, right) => left.position - right.position);

        for (const type of ['text', 'voice']) {
            const group = channels.filter(channel => channel.type === type);

            if (! group.length) {
                continue;
            }

            const heading = document.createElement('p');

            heading.className = 'label-mono mt-3 px-1 first:mt-0';
            heading.textContent = type === 'text' ? 'Canais de texto' : 'Canais de voz';
            list.appendChild(heading);

            for (const channel of group) {
                const button = document.createElement('button');
                const active = channel.id === this.channel?.id;
                const inVoice = channel.id === this.voice.channel?.id;

                button.type = 'button';
                button.className = 'row-item mt-1.5 w-full cursor-pointer text-left text-[13px] '
                    + (active ? 'row-item-on text-white' : inVoice ? 'text-online' : 'text-ink-soft');
                button.innerHTML = '<span class="w-3 shrink-0 text-center font-mono text-[11px] text-lilac-2"></span><span class="min-w-0 flex-1 truncate"></span>';
                button.querySelector('span').textContent = type === 'text' ? '#' : '🔊';
                button.querySelectorAll('span')[1].textContent = channel.name;
                button.onclick = () => void this.attempt(() => this.openChannel(channel));
                list.appendChild(button);

                for (const person of this.tree.voice?.[channel.id] ?? []) {
                    const row = document.createElement('p');
                    const icons = { mic: person.muted ? '🔇' : '🎙️', screen: '🖥️', camera: '📷' };

                    row.className = 'ml-6 flex items-center gap-1.5 truncate px-2 py-0.5 text-[13px] text-ink';
                    row.innerHTML = '<span class="avatar size-5 shrink-0 text-[10px]"></span><span class="truncate"></span><span class="label-mono"></span>';
                    row.querySelector('span').textContent = (person.name ?? '?').slice(0, 1).toUpperCase();
                    row.querySelectorAll('span')[1].textContent = person.name;
                    row.querySelectorAll('span')[2].textContent = (person.sources ?? []).map(source => icons[source] ?? '').join('');
                    list.appendChild(row);
                }
            }
        }
    }

    /** Membros agrupados pelo cargo mais alto, com a cor dele; sem cargo, "Membros". */
    drawMembers() {
        const list = el('member-list');

        list.innerHTML = '';

        if (! this.tree) {
            return;
        }

        const roles = [...this.tree.roles].filter(role => ! role.is_everyone).sort((left, right) => right.position - left.position);
        const groups = new Map([...roles.map(role => [role.id, []]), ['none', []]]);

        for (const member of this.tree.members) {
            const top = roles.find(role => member.role_ids.includes(role.id));

            groups.get(top?.id ?? 'none').push(member);
        }

        for (const [roleId, members] of groups) {
            if (! members.length) {
                continue;
            }

            const role = roles.find(candidate => candidate.id === roleId);
            const heading = document.createElement('p');

            heading.className = 'label-mono mt-3 px-1 first:mt-0';
            heading.textContent = `${role?.name ?? 'Membros'} — ${members.length}`;
            list.appendChild(heading);

            for (const member of members.sort((left, right) => (left.nickname ?? left.name).localeCompare(right.nickname ?? right.name))) {
                const row = document.createElement('button');

                row.type = 'button';
                row.dataset.user = member.user_id;
                row.className = 'row-item group mt-1 w-full cursor-pointer text-left text-[13px]';
                row.innerHTML = '<span class="avatar relative size-7 shrink-0 text-xs" data-avatar>'
                    + '<span class="absolute -bottom-0.5 -right-0.5 size-2.5 rounded-full border-2 border-back" data-presence></span>'
                    + '</span>'
                    + '<span class="size-[7px] shrink-0 rounded-full bg-ink-dim" data-role-dot></span>'
                    + '<span class="min-w-0 flex-1 truncate" data-member-name></span>'
                    + '<span class="label-mono" data-member-state></span>'
                    + '<span class="hidden text-ink-soft group-hover:inline">⋯</span>';
                row.querySelector('[data-avatar]').firstChild.textContent = (member.nickname ?? member.name).slice(0, 1).toUpperCase();
                row.querySelector('[data-member-name]').textContent = member.nickname ?? member.name;

                // A cor do cargo vai no ponto, não no nome: cargo escuro deixava o nome ilegível.
                row.querySelector('[data-role-dot]').style.background = role?.color ?? '';
                row.querySelector('[data-member-state]').textContent = [member.is_owner ? '👑' : '', member.server_mute ? '🔇' : '', member.server_deaf ? '🙉' : ''].join('');
                row.onclick = event => this.openMemberMenu(member, event);
                row.oncontextmenu = event => {
                    event.preventDefault();
                    this.openMemberMenu(member, event);
                };
                list.appendChild(row);
                this.paintPresence(member.user_id);
            }
        }
    }

    /** Só a linha de quem entrou ou saiu: a lista inteira só se redesenha com a árvore. */
    paintPresence(userId) {
        const row = el('member-list').querySelector(`[data-user="${userId}"]`);

        if (! row) {
            return;
        }

        const online = this.online.has(userId);

        // Só o avatar esmaece: o nome fica inteiro, com a cor do cargo.
        row.querySelector('[data-avatar]').classList.toggle('opacity-50', ! online);
        row.querySelector('[data-presence]').classList.toggle('bg-online', online);
        row.querySelector('[data-presence]').classList.toggle('bg-ink-dim', ! online);
    }

    async openChannel(channel) {
        this.channel = channel;
        this.drawChannels();
        this.drawCenter();

        if (! channel) {
            this.chat.close();

            return;
        }

        if (channel.type === 'text') {
            await this.chat.open(channel);

            return;
        }

        this.chat.close();

        if (Permissions.has(channel.permissions, Permissions.CONNECT)) {
            await this.voice.join(channel);
        } else {
            this.app.toast('você não pode entrar neste canal de voz', true);
        }
    }

    /** O centro é chat, palco ou nada — conforme o canal escolhido, não a voz ativa. */
    drawCenter() {
        const channel = this.channel;
        const text = channel?.type === 'text';

        el('hub-home').hidden = Boolean(this.tree);
        el('hub-empty').hidden = Boolean(channel) || ! this.tree;
        el('chat').hidden = ! text;
        el('stage-host').hidden = ! (channel && ! text && this.voice.channel);
        el('channel-kind').textContent = channel ? (text ? '#' : '🔊') : '';
        el('channel-title').textContent = channel?.name ?? '';
        el('channel-topic').textContent = channel?.topic ?? '';
        el('channel-edit').hidden = ! (channel && this.can(Permissions.MANAGE_CHANNELS));

        // O palco atrás do chat continuava decodificando: quem assiste só vê o texto.
        this.app.paintWatching();
        this.voice.paintBar();
    }

    wireMemberMenu() {
        const patch = body => this.attempt(async () => {
            await this.api.patch(`/api/servers/${this.tree.id}/members/${this.menuTarget.user_id}`, body);
            el('member-menu').hidden = true;
        });

        el('member-menu-nickname-form').onsubmit = event => {
            event.preventDefault();
            void patch({ nickname: el('member-menu-nickname').value.trim() || null });
        };
        el('member-menu-mute').onclick = () => void patch({ server_mute: ! this.menuTarget.server_mute });
        el('member-menu-deafen').onclick = () => void patch({ server_deaf: ! this.menuTarget.server_deaf });
        el('member-menu-disconnect').onclick = () => void this.attempt(async () => {
            const channelId = Object.keys(this.tree.voice).find(id => this.tree.voice[id].some(person => person.user_id === this.menuTarget.user_id));

            await this.api.delete(`/api/channels/${channelId}/voice/members/${this.menuTarget.user_id}`);
            el('member-menu').hidden = true;
        });
        el('member-menu-kick').onclick = () => {
            if (! confirm(`Expulsar ${this.menuTarget.name} do servidor?`)) {
                return;
            }

            void this.attempt(async () => {
                await this.api.delete(`/api/servers/${this.tree.id}/members/${this.menuTarget.user_id}`);
                el('member-menu').hidden = true;
            });
        };
        el('member-menu-ban').onclick = () => {
            el('member-menu-ban-form').hidden = false;
            el('member-menu-ban-reason').focus();
        };
        el('member-menu-ban-form').onsubmit = event => {
            event.preventDefault();

            if (! confirm(`Banir ${this.menuTarget.name}? A pessoa não consegue voltar até ser perdoada.`)) {
                return;
            }

            void this.attempt(async () => {
                await this.api.post(`/api/servers/${this.tree.id}/bans/${this.menuTarget.user_id}`, { reason: el('member-menu-ban-reason').value.trim() || undefined });
                el('member-menu').hidden = true;
            });
        };
    }

    openMemberMenu(member, event) {
        const menu = el('member-menu');
        const self = member.user_id === this.user.id;
        const below = Permissions.outranks(this.tree, this.me(), member) && ! self;
        const inVoice = Object.values(this.tree.voice ?? {}).some(people => people.some(person => person.user_id === member.user_id));

        this.menuTarget = member;
        el('member-menu-name').textContent = member.nickname ? `${member.nickname} (${member.name})` : member.name;
        el('member-menu-nickname').value = member.nickname ?? '';
        el('member-menu-nickname-form').hidden = ! (self || (below && this.can(Permissions.MANAGE_SERVER)));
        el('member-menu-mute').hidden = ! (below && this.can(Permissions.MUTE_MEMBERS));
        el('member-menu-mute').textContent = member.server_mute ? 'Desmutar no servidor' : 'Mutar no servidor';
        el('member-menu-deafen').hidden = ! (below && this.can(Permissions.DEAFEN_MEMBERS));
        el('member-menu-deafen').textContent = member.server_deaf ? 'Devolver o áudio' : 'Ensurdecer no servidor';
        el('member-menu-disconnect').hidden = ! (below && inVoice && this.can(Permissions.MOVE_MEMBERS));
        el('member-menu-kick').hidden = ! (below && this.can(Permissions.KICK_MEMBERS));
        el('member-menu-ban').hidden = ! (below && this.can(Permissions.BAN_MEMBERS));
        el('member-menu-ban-form').hidden = true;
        el('member-menu-ban-reason').value = '';
        this.drawMenuRoles(member, below && this.can(Permissions.MANAGE_ROLES));

        menu.hidden = false;
        menu.style.left = `${Math.min(event.clientX, window.innerWidth - 272)}px`;
        menu.style.top = `${Math.min(event.clientY, window.innerHeight - menu.offsetHeight - 8)}px`;
        event.stopPropagation();
    }

    /** Só os cargos abaixo do meu: dar um acima seria promover alguém acima de mim. */
    drawMenuRoles(member, allowed) {
        const box = el('member-menu-roles');
        const myTop = this.tree.me.top_position;

        box.innerHTML = '';
        box.hidden = ! allowed;

        if (! allowed) {
            return;
        }

        for (const role of this.tree.roles.filter(role => ! role.is_everyone && role.position < myTop)) {
            const label = document.createElement('label');

            label.className = 'flex cursor-pointer items-center gap-2 py-0.5 text-sm text-ink';
            label.innerHTML = '<input class="accent-brand" type="checkbox"><span></span>';
            label.querySelector('input').checked = member.role_ids.includes(role.id);
            label.querySelector('span').textContent = role.name;
            label.querySelector('span').style.color = role.color ?? '';
            label.querySelector('input').onchange = event => {
                const roleIds = event.target.checked
                    ? [...new Set([...member.role_ids, role.id])]
                    : member.role_ids.filter(id => id !== role.id);

                void this.attempt(() => this.api.patch(`/api/servers/${this.tree.id}/members/${member.user_id}`, { role_ids: roleIds }));
            };
            box.appendChild(label);
        }
    }
}
