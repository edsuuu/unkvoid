export type User = {
    id: number;
    name: string;
    email: string;
    avatar_url: string | null;
    admin: boolean;
};

export type AuthToken = {
    token: string;
    user?: User;
};

export type ServerSummary = {
    id: number;
    name: string;
    owner_id: number;
    icon_url: string | null;
    last_accessed_at: string | null;
};

export type ChannelType = 'text' | 'voice';

export type OverwriteTargetType = 'role' | 'member';

export type Overwrite = {
    target_type: OverwriteTargetType;
    target_id: number;
    allow: number;
    deny: number;
};

export type Channel = {
    id: string;
    name: string;
    type: ChannelType;
    topic: string | null;
    position: number;
    user_limit: number | null;
    permissions: number;
    overwrites: Overwrite[];
};

export type Role = {
    id: number;
    name: string;
    color: string | null;
    position: number;
    permissions: number;
    is_everyone: boolean;
};

export type Member = {
    user_id: number;
    name: string;
    avatar_url: string | null;
    nickname: string | null;
    role_ids: number[];
    server_mute: boolean;
    server_deaf: boolean;
    is_owner: boolean;
};

export type Ban = {
    user_id: number;
    name: string;
    reason: string | null;
    banned_by: number | null;
    created_at: string;
};

export type VoicePerson = {
    user_id: number;
    name: string;
    sources: string[];
    muted?: boolean;
};

export type ServerTree = {
    id: number;
    name: string;
    owner_id: number;
    invite_code: string | null;
    icon_url: string | null;
    me: { user_id: number; permissions: number; top_position: number };
    roles: Role[];
    channels: Channel[];
    members: Member[];
    voice: Record<string, VoicePerson[]>;
    bans: Ban[];
};

export type MessageType = 'user' | 'join';

export type ReplyTo = {
    id: number;
    name: string;
    body: string;
};

export type Message = {
    id: number;
    channel_id: string;
    type: MessageType;
    user: { id: number; name: string; avatar_url: string | null };
    reply_to: ReplyTo | null;
    body: string;
    edited_at: string | null;
    created_at: string;
};

export type ClipStatus = 'processing' | 'ready' | 'failed';

export type Clip = {
    id: string;
    status: ClipStatus;
    streamer: { id: number | null; name: string };
    server_name: string;
    channel_name: string;
    duration_ms: number | null;
    size_bytes: number | null;
    created_at: string;
    expires_at: string;
    thumbnail_url: string | null;
    playlist_url: string | null;
    download_url: string | null;
};

export type Config = {
    sfu: string;
    reverb: { host: string; port: number; key: string; scheme: string };
};

export type Person = {
    id: number;
    name: string;
    avatar_url: string | null;
};

export type FriendshipStatus = 'pending' | 'accepted' | 'blocked';

export type Friendship = {
    id: number;
    status: FriendshipStatus;
    requester: Person;
    addressee: Person;
    responded_at: string | null;
    created_at: string | null;
};

export type DirectMessage = {
    id: number;
    body: string;
    created_at: string;
    edited_at: string | null;
    mine: boolean;
    sender: Person;
};

export type DirectConversation = {
    user: Person;
    last: { id: number; body: string; created_at: string; mine: boolean } | null;
    unread: number;
};

export type Audit = {
    id: string;
    at: string;
    event: string;
    type: string;
    actor: Person | null;
    summary: string;
};
