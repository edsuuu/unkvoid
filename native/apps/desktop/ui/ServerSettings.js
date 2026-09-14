import { Permissions } from './Permissions.js';

const el = id => document.getElementById(id);

const BUTTON = 'btn-ghost px-2 py-1 text-xs';

/**
 * Os modais do servidor: nome, convite, cargos, banimentos e o editor de canal com o
 * "Ocultar canal". Tudo lê a árvore do `Hub` e grava pela API dele; a árvore volta
 * refeita pelo `ServerUpdated`.
 */
export class ServerSettings {
    constructor(hub) {
        this.hub = hub;

        el('settings-close').onclick = () => { el('settings-modal').hidden = true; };
        el('channel-cancel').onclick = () => { el('channel-modal').hidden = true; };
        el('role-cancel').onclick = () => { el('role-modal').hidden = true; };
    }

    get tree() {
        return this.hub.tree;
    }

    get api() {
        return this.hub.api;
    }

    attempt(work) {
        return this.hub.attempt(work);
    }

    can(flag) {
        return this.hub.can(flag);
    }

    open() {
        const modal = el('settings-modal');

        el('settings-name').value = this.tree.name;
        el('settings-name-form').onsubmit = event => {
            event.preventDefault();
            void this.attempt(() => this.api.patch(`/api/servers/${this.tree.id}`, { name: el('settings-name').value.trim() }));
        };
        el('settings-name-form').hidden = ! this.can(Permissions.MANAGE_SERVER);

        el('settings-invite-block').hidden = ! this.tree.invite_code;
        el('settings-invite').textContent = this.tree.invite_code ?? '';
        el('settings-invite-copy').onclick = () => void navigator.clipboard.writeText(this.tree.invite_code).then(() => this.hub.app.toast('convite copiado'));
        el('settings-invite-regenerate').onclick = () => void this.attempt(async () => {
            const { invite_code: inviteCode } = await this.api.post(`/api/servers/${this.tree.id}/invite`);

            this.tree.invite_code = inviteCode;
            el('settings-invite').textContent = inviteCode;
        });

        el('settings-roles-block').hidden = ! this.can(Permissions.MANAGE_ROLES);
        el('settings-role-add').onclick = () => this.openRoleModal(null);
        this.drawRoles();

        el('settings-bans-block').hidden = ! this.can(Permissions.BAN_MEMBERS);
        this.drawBans();

        const owner = this.tree.owner_id === this.hub.user.id;

        el('settings-delete').hidden = ! owner;
        el('settings-leave').hidden = owner;
        el('settings-delete').onclick = () => {
            if (! confirm(`Apagar o servidor "${this.tree.name}"? Isso não tem volta.`)) {
                return;
            }

            void this.attempt(async () => {
                await this.api.delete(`/api/servers/${this.tree.id}`);
                modal.hidden = true;
                await this.hub.closeServer();
                await this.hub.loadServers();
            });
        };
        el('settings-leave').onclick = () => void this.attempt(async () => {
            await this.api.post(`/api/servers/${this.tree.id}/leave`);
            modal.hidden = true;
            await this.hub.closeServer();
            await this.hub.loadServers();
        });

        modal.hidden = false;
        el('settings-name').focus();
    }

    drawRoles() {
        const list = el('settings-roles');
        const roles = [...this.tree.roles].sort((left, right) => right.position - left.position);
        const myTop = this.tree.me.top_position;

        list.innerHTML = '';

        roles.forEach((role, index) => {
            const row = document.createElement('div');
            const editable = role.is_everyone || role.position < myTop;

            row.className = 'row-item text-sm';
            row.innerHTML = '<span class="size-[7px] shrink-0 rounded-full bg-ink-dim"></span>'
                + '<span class="min-w-0 flex-1 truncate text-white"></span>'
                + `<button class="${BUTTON}" data-role-up type="button" title="Subir">▲</button>`
                + `<button class="${BUTTON}" data-role-down type="button" title="Descer">▼</button>`
                + `<button class="${BUTTON}" data-role-edit type="button">Editar</button>`
                + `<button class="${BUTTON} text-danger" data-role-delete type="button">Apagar</button>`;
            row.querySelector('span').style.background = role.color ?? '';
            row.querySelectorAll('span')[1].textContent = `${role.name} · ${role.position}`;

            const above = roles[index - 1];
            const under = roles[index + 1];

            // Trocar de lugar com o vizinho: a posição do outro vira a minha. O servidor
            // é quem reordena o resto.
            row.querySelector('[data-role-up]').hidden = ! (editable && above && ! above.is_everyone && above.position < myTop);
            row.querySelector('[data-role-down]').hidden = ! (editable && under && ! under.is_everyone && ! role.is_everyone);
            row.querySelector('[data-role-up]').onclick = () => void this.attempt(() => this.api.patch(`/api/roles/${role.id}`, { position: above.position }));
            row.querySelector('[data-role-down]').onclick = () => void this.attempt(() => this.api.patch(`/api/roles/${role.id}`, { position: under.position }));
            row.querySelector('[data-role-edit]').hidden = ! editable;
            row.querySelector('[data-role-edit]').onclick = () => this.openRoleModal(role);
            row.querySelector('[data-role-delete]').hidden = ! editable || role.is_everyone;
            row.querySelector('[data-role-delete]').onclick = () => this.deleteRole(role);
            list.appendChild(row);
        });
    }

    deleteRole(role) {
        if (confirm(`Apagar o cargo "${role.name}"?`)) {
            void this.attempt(async () => {
                await this.api.delete(`/api/roles/${role.id}`);
                el('role-modal').hidden = true;
            });
        }
    }

    drawBans() {
        const list = el('settings-bans');

        list.innerHTML = '';

        if (! this.tree.bans?.length) {
            list.innerHTML = '<p class="text-sm text-ink-dim">Ninguém banido.</p>';

            return;
        }

        for (const ban of this.tree.bans) {
            const row = document.createElement('div');

            row.className = 'row-item text-sm';
            row.innerHTML = '<span class="min-w-0 flex-1 truncate text-white"></span>'
                + '<span class="min-w-0 flex-1 truncate text-xs text-ink-soft"></span>'
                + `<button class="${BUTTON}" data-unban type="button">Perdoar</button>`;
            row.querySelector('span').textContent = ban.name;
            row.querySelectorAll('span')[1].textContent = ban.reason ?? '';
            row.querySelector('[data-unban]').onclick = () => void this.attempt(() => this.api.delete(`/api/servers/${this.tree.id}/bans/${ban.user_id}`));
            list.appendChild(row);
        }
    }

    openRoleModal(role) {
        const box = el('role-permissions');
        const everyone = Boolean(role?.is_everyone);

        el('role-title').textContent = role ? `Cargo: ${role.name}` : 'Novo cargo';
        el('role-name').value = role?.name ?? '';
        el('role-name').disabled = everyone;
        el('role-color').value = role?.color ?? '#8a7cf5';
        el('role-color').disabled = everyone;
        el('role-delete').hidden = ! role || everyone;
        box.innerHTML = '';

        for (const [flag, label] of Permissions.LABELS) {
            const item = document.createElement('label');

            item.className = 'flex cursor-pointer items-center gap-2';
            item.innerHTML = '<input class="accent-brand" type="checkbox" data-permission><span></span>';
            item.querySelector('input').value = flag;
            item.querySelector('input').checked = ((role?.permissions ?? 0) & Permissions[flag]) !== 0;
            item.querySelector('span').textContent = label;
            box.appendChild(item);
        }

        el('role-form').onsubmit = event => {
            event.preventDefault();

            const permissions = [...box.querySelectorAll('[data-permission]')]
                .filter(input => input.checked)
                .reduce((bits, input) => bits | Permissions[input.value], 0);
            const body = everyone
                ? { permissions }
                : { name: el('role-name').value.trim(), color: el('role-color').value, permissions };

            void this.attempt(async () => {
                await (role ? this.api.patch(`/api/roles/${role.id}`, body) : this.api.post(`/api/servers/${this.tree.id}/roles`, body));
                el('role-modal').hidden = true;
            });
        };
        el('role-delete').onclick = () => this.deleteRole(role);

        el('role-modal').hidden = false;
        el('role-name').focus();
    }

    openChannelModal(channel) {
        el('channel-modal-title').textContent = channel ? `Canal: ${channel.name}` : 'Novo canal';
        el('channel-name').value = channel?.name ?? '';
        el('channel-type').value = channel?.type ?? 'text';
        el('channel-type').disabled = Boolean(channel);
        el('channel-topic').value = channel?.topic ?? '';
        el('channel-limit').value = channel?.user_limit ?? '';
        el('channel-delete').hidden = ! channel;
        el('channel-overwrites-block').hidden = ! (channel && this.can(Permissions.MANAGE_ROLES));

        if (channel && this.can(Permissions.MANAGE_ROLES)) {
            this.drawOverwrites(channel);
        }

        el('channel-form').onsubmit = event => {
            event.preventDefault();

            const limit = el('channel-limit').value === '' ? null : Number(el('channel-limit').value);
            const body = {
                name: el('channel-name').value.trim(),
                topic: el('channel-topic').value.trim() || null,
                user_limit: limit,
            };

            void this.attempt(async () => {
                await (channel
                    ? this.api.patch(`/api/channels/${channel.id}`, body)
                    : this.api.post(`/api/servers/${this.tree.id}/channels`, { ...body, type: el('channel-type').value }));
                el('channel-modal').hidden = true;
            });
        };
        el('channel-delete').onclick = () => {
            if (! confirm(`Apagar o canal "${channel.name}"? As mensagens vão junto.`)) {
                return;
            }

            void this.attempt(async () => {
                await this.api.delete(`/api/channels/${channel.id}`);
                el('channel-modal').hidden = true;
            });
        };

        el('channel-modal').hidden = false;
        el('channel-name').focus();
    }

    /**
     * "Ocultar canal": uma linha por cargo ou membro com sobrescrita, seis células
     * de três estados. Cada clique grava na hora — não há "salvar" para esquecer.
     */
    drawOverwrites(channel) {
        const list = el('channel-overwrites');
        const select = el('channel-overwrite-target');
        const overwrites = channel.overwrites ?? [];
        const targets = [
            ...this.tree.roles.map(role => ({ type: 'role', id: role.id, name: role.name, color: role.color })),
            ...this.tree.members.map(member => ({ type: 'member', id: member.user_id, name: member.nickname ?? member.name, color: null })),
        ];
        const shown = targets.filter(target => target.type === 'role' && this.tree.roles.find(role => role.id === target.id)?.is_everyone
            || overwrites.some(item => item.target_type === target.type && item.target_id === target.id));

        list.innerHTML = '';
        select.innerHTML = '<option value="">Adicionar cargo ou membro…</option>';

        for (const target of targets.filter(target => ! shown.includes(target))) {
            const option = document.createElement('option');

            option.value = `${target.type}:${target.id}`;
            option.textContent = `${target.type === 'role' ? 'cargo' : 'membro'}: ${target.name}`;
            select.appendChild(option);
        }

        select.onchange = () => {
            const [type, id] = select.value.split(':');

            if (! type) {
                return;
            }

            const target = targets.find(candidate => candidate.type === type && String(candidate.id) === id);

            channel.overwrites = [...overwrites, { target_type: type, target_id: target.id, allow: 0, deny: 0 }];
            this.drawOverwrites(channel);
        };

        const header = document.createElement('div');

        header.className = 'label-mono grid grid-cols-[1fr_repeat(6,2.5rem)] gap-1 px-2';
        header.innerHTML = '<span></span>' + Permissions.OVERWRITABLE.map(flag => `<span class="text-center" title="${flag}">${{ VIEW_CHANNEL: 'ver', SEND_MESSAGES: 'falar', CONNECT: 'entrar', SPEAK: 'voz', STREAM: 'tela', VIDEO: 'cam' }[flag]}</span>`).join('');
        list.appendChild(header);

        for (const target of shown) {
            const item = overwrites.find(candidate => candidate.target_type === target.type && candidate.target_id === target.id) ?? { allow: 0, deny: 0 };
            const row = document.createElement('div');

            row.className = 'row-item grid grid-cols-[1fr_repeat(6,2.5rem)] gap-1 text-sm';
            row.innerHTML = '<span class="truncate text-white"></span>';
            row.querySelector('span').textContent = target.name;
            row.querySelector('span').style.color = target.color ?? '';

            for (const flag of Permissions.OVERWRITABLE) {
                const bit = Permissions[flag];
                const cell = document.createElement('button');
                const state = (item.allow & bit) ? 'allow' : (item.deny & bit) ? 'deny' : 'inherit';

                cell.type = 'button';
                cell.dataset.overwrite = flag;
                cell.className = 'cursor-pointer rounded py-0.5 text-center text-xs '
                    + { allow: 'bg-online text-back', deny: 'bg-danger text-back', inherit: 'bg-row text-ink-soft' }[state];
                cell.textContent = { allow: '✓', deny: '✕', inherit: '—' }[state];
                cell.onclick = () => {
                    const next = state === 'inherit' ? 'allow' : state === 'allow' ? 'deny' : 'inherit';
                    const allow = next === 'allow' ? item.allow | bit : item.allow & ~bit;
                    const deny = next === 'deny' ? item.deny | bit : item.deny & ~bit;

                    void this.attempt(async () => {
                        const path = `/api/channels/${channel.id}/overwrites/${target.type}/${target.id}`;

                        await (allow || deny ? this.api.put(path, { allow, deny }) : this.api.delete(path));
                        channel.overwrites = [...overwrites.filter(candidate => candidate !== item), ...(allow || deny ? [{ target_type: target.type, target_id: target.id, allow, deny }] : [])];
                        this.drawOverwrites(channel);
                    });
                };
                row.appendChild(cell);
            }

            list.appendChild(row);
        }
    }
}
