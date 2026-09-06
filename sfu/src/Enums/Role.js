export const Role = Object.freeze({
    Owner: 'owner',
    Admin: 'admin',
    Member: 'member',
});

export const canModerate = role => role === Role.Owner || role === Role.Admin;
