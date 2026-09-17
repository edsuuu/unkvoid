import { Field } from './Field.ts';
import type { Hub } from './Hub.ts';
import type { Audit, Ban, Channel, ChannelType, Overwrite, OverwriteTargetType, Role, ServerSummary, ServerTree } from './Models.ts';
import { Permissions, type PermissionName } from './Permissions.ts';
import { Store } from './Store.ts';

export type AuditState = {
    entries: Audit[];
    loading: boolean;
    failed: boolean;
};

export type RoleRow = { role: Role; editable: boolean; up: number | null; down: number | null };

export type OverwriteTarget = { type: OverwriteTargetType; id: number; name: string; color: string | null; everyone: boolean };

export type ShownOverwriteTarget = OverwriteTarget & { allow: number; deny: number };

export type RoleDraft = { name: string; color: string; permissions: number };

export type ChannelDraft = { name: string; type: ChannelType; topic: string; limit: string };

export class ServerSettings {
    static readonly OVERWRITE_LABELS: Partial<Record<PermissionName, string>> = { VIEW_CHANNEL: 'ver', SEND_MESSAGES: 'falar', CONNECT: 'entrar', SPEAK: 'voz', STREAM: 'tela', VIDEO: 'cam' };

    static readonly ICON_MAX_BYTES = 2 * 1024 * 1024;

    readonly hub: Hub;
    readonly audits: Store<AuditState>;

    constructor(hub: Hub) {
        this.hub = hub;
        this.audits = new Store<AuditState>({ entries: [], loading: false, failed: false });
    }

    private get tree(): ServerTree {
        return this.hub.tree!;
    }

    rename(name: string): Promise<unknown> {
        const tree = this.tree;

        return this.hub.attempt(() => {
            Field.require(name, 'Dê um nome ao servidor.');

            return this.hub.api.patch(`/api/servers/${tree.id}`, { name: name.trim() });
        });
    }

    async loadAudits(): Promise<void> {
        this.audits.set({ entries: [], loading: true, failed: false });

        const page = await this.hub.attempt(() => this.hub.api.get<{ data: Audit[] } | Audit[]>(`/api/servers/${this.tree.id}/audits`));
        const entries = Array.isArray(page) ? page : page?.data;

        this.audits.set(entries ? { entries, loading: false } : { loading: false, failed: true });
    }

    async uploadIcon(file: File): Promise<void> {
        if (file.size > ServerSettings.ICON_MAX_BYTES) {
            this.hub.app.toast('o ícone precisa ter menos de 2 MB', true);

            return;
        }

        const updated = await this.hub.attempt(() => this.hub.api.upload<ServerSummary>(`/api/servers/${this.tree.id}/icon`, 'icon', file));

        if (! updated) {
            return;
        }

        this.hub.tree = { ...this.tree, icon_url: updated.icon_url };
        this.hub.publish();
        await this.hub.loadServers();
    }

    async removeIcon(): Promise<void> {
        const removed = await this.hub.attempt(async () => {
            await this.hub.api.delete(`/api/servers/${this.tree.id}/icon`);

            return true;
        });

        if (! removed) {
            return;
        }

        this.hub.tree = { ...this.tree, icon_url: null };
        this.hub.publish();
        await this.hub.loadServers();
    }

    regenerateInvite(): Promise<void> {
        return this.hub.attempt(async () => {
            const { invite_code: inviteCode } = await this.hub.api.post<{ invite_code: string }>(`/api/servers/${this.tree.id}/invite`);

            this.hub.tree = { ...this.tree, invite_code: inviteCode };
            this.hub.publish();
        });
    }

    async deleteServer(): Promise<void> {
        const tree = this.tree;

        if (! await this.hub.app.confirm(`Apagar o servidor "${tree.name}"? Isso não tem volta.`, 'Apagar')) {
            return;
        }

        await this.hub.attempt(async () => {
            await this.hub.api.delete(`/api/servers/${tree.id}`);
            await this.hub.closeServer();
            await this.hub.loadServers();
        });
    }

    async leaveServer(): Promise<void> {
        const tree = this.tree;

        if (! await this.hub.app.confirm(`Sair do servidor "${tree.name}"? Para voltar, só com um convite.`, 'Sair')) {
            return;
        }

        await this.hub.attempt(async () => {
            await this.hub.api.post(`/api/servers/${tree.id}/leave`);
            await this.hub.closeServer();
            await this.hub.loadServers();
        });
    }

    roleRows(): RoleRow[] {
        const tree = this.tree;
        const roles = [...tree.roles].sort((left, right) => right.position - left.position);
        const myTop = tree.me.top_position;

        return roles.map((role, index) => {
            const above = roles[index - 1];
            const under = roles[index + 1];
            const editable = role.is_everyone || role.position < myTop;

            return {
                role,
                editable,
                up: editable && ! role.is_everyone && above && ! above.is_everyone && above.position < myTop ? above.position : null,
                down: editable && under && ! under.is_everyone && ! role.is_everyone ? under.position : null,
            };
        });
    }

    moveRole(role: Role, position: number): Promise<unknown> {
        return this.hub.attempt(() => this.hub.api.patch(`/api/roles/${role.id}`, { position }));
    }

    async saveRole(role: Role | null, { name, color, permissions }: RoleDraft): Promise<void> {
        const body = role?.is_everyone ? { permissions } : { name: name.trim(), color, permissions };

        const saved = await this.hub.attempt(async () => {
            if (! role?.is_everyone) {
                Field.require(name, 'Dê um nome ao cargo.');
            }

            await (role ? this.hub.api.patch(`/api/roles/${role.id}`, body) : this.hub.api.post(`/api/servers/${this.tree.id}/roles`, body));

            return true;
        });

        if (saved) {
            this.hub.store.set({ roleEditor: null });
        }
    }

    async deleteRole(role: Role): Promise<void> {
        if (! await this.hub.app.confirm(`Apagar o cargo "${role.name}"?`, 'Apagar')) {
            return;
        }

        const removed = await this.hub.attempt(async () => {
            await this.hub.api.delete(`/api/roles/${role.id}`);

            return true;
        });

        if (removed) {
            this.hub.store.set({ roleEditor: null });
        }
    }

    unban(ban: Ban): Promise<unknown> {
        return this.hub.attempt(() => this.hub.api.delete(`/api/servers/${this.tree.id}/bans/${ban.user_id}`));
    }

    async saveChannel(channel: Channel | null, { name, type, topic, limit }: ChannelDraft): Promise<void> {
        const body = { name: name.trim(), topic: topic.trim() || null, user_limit: limit === '' ? null : Number(limit) };

        const saved = await this.hub.attempt(async () => {
            Field.require(name, 'Dê um nome ao canal.');

            if (body.user_limit !== null && ! (Number.isInteger(body.user_limit) && body.user_limit >= 1 && body.user_limit <= 99)) {
                throw new Error('O limite de pessoas vai de 1 a 99. Vazio é sem limite.');
            }

            await (channel
                ? this.hub.api.patch(`/api/channels/${channel.id}`, body)
                : this.hub.api.post(`/api/servers/${this.tree.id}/channels`, { ...body, type }));

            return true;
        });

        if (saved) {
            this.hub.closeModal();
        }
    }

    async deleteChannel(channel: Channel): Promise<void> {
        if (! await this.hub.app.confirm(`Apagar o canal "${channel.name}"? As mensagens vão junto.`, 'Apagar')) {
            return;
        }

        const removed = await this.hub.attempt(async () => {
            await this.hub.api.delete(`/api/channels/${channel.id}`);

            return true;
        });

        if (removed) {
            this.hub.closeModal();
        }
    }

    overwriteTargets(overwrites: Overwrite[]): { shown: ShownOverwriteTarget[]; available: OverwriteTarget[] } {
        const tree = this.tree;
        const targets: OverwriteTarget[] = [
            ...tree.roles.map(role => ({ type: 'role' as const, id: role.id, name: role.name, color: role.color, everyone: role.is_everyone })),
            ...tree.members.map(member => ({ type: 'member' as const, id: member.user_id, name: member.nickname ?? member.name, color: null, everyone: false })),
        ];
        const shown: ShownOverwriteTarget[] = [];
        const available: OverwriteTarget[] = [];

        for (const target of targets) {
            const item = overwrites.find(candidate => candidate.target_type === target.type && candidate.target_id === target.id);

            if (target.everyone || item) {
                shown.push({ ...target, allow: item?.allow ?? 0, deny: item?.deny ?? 0 });
            } else {
                available.push(target);
            }
        }

        return { shown, available };
    }

    async cycleOverwrite(channel: Channel, overwrites: Overwrite[], target: ShownOverwriteTarget, flag: PermissionName): Promise<Overwrite[] | null> {
        const bit = Permissions[flag];
        const state = (target.allow & bit) ? 'allow' : (target.deny & bit) ? 'deny' : 'inherit';
        const next = state === 'inherit' ? 'allow' : state === 'allow' ? 'deny' : 'inherit';
        const allow = next === 'allow' ? target.allow | bit : target.allow & ~bit;
        const deny = next === 'deny' ? target.deny | bit : target.deny & ~bit;
        const path = `/api/channels/${channel.id}/overwrites/${target.type}/${target.id}`;

        const saved = await this.hub.attempt(async () => {
            await (allow || deny ? this.hub.api.put(path, { allow, deny }) : this.hub.api.delete(path));

            return true;
        });

        if (! saved) {
            return null;
        }

        const others = overwrites.filter(item => ! (item.target_type === target.type && item.target_id === target.id));

        return allow || deny ? [...others, { target_type: target.type, target_id: target.id, allow, deny }] : others;
    }
}
