export const Role = {
    Owner: 'owner',
    Admin: 'admin',
    Member: 'member',
} as const;

export type RoleName = (typeof Role)[keyof typeof Role];

export const canModerate = (role: RoleName): boolean => role === Role.Owner || role === Role.Admin;
