import { useState, type FormEvent } from 'react';

import { Spinner } from '../common/Spinner.tsx';
import { useApp } from '../useApp.ts';
import { useStore } from '../useStore.ts';

export function AuthCard() {
    const hub = useApp().hub;
    const { loginMode, loginError, loginBusy, googleWaiting } = useStore(hub.store);
    const [name, setName] = useState('');
    const [email, setEmail] = useState('');
    const [password, setPassword] = useState('');
    const registering = loginMode === 'register';

    const submit = (event: FormEvent<HTMLFormElement>) => {
        event.preventDefault();
        void (registering ? hub.register(name, email, password) : hub.login(email, password));
    };

    return (
        <div className="glass flex w-full max-w-[420px] animate-rise flex-col justify-center rounded-[22px] p-8 shadow-[0_30px_80px_-40px_rgba(0,0,0,0.9)]">
            <div key={loginMode} className="animate-rise">
                <p className="text-center text-base font-semibold">{registering ? 'Criar conta' : 'Entrar'}</p>
                <p className="mt-1.5 mb-5 text-center text-[12.5px] text-ink-soft">
                    {registering ? 'Para ter servidores, voz, chat e clipes.' : 'O login é opcional. Dá para usar tudo sem conta.'}
                </p>

                <button
                    className="flex w-full cursor-pointer items-center justify-center gap-2.5 rounded-xl bg-white p-3 text-sm font-semibold text-[#1f1f1f] transition hover:brightness-95 disabled:opacity-60"
                    type="button"
                    disabled={googleWaiting}
                    onClick={() => void hub.googleLogin()}
                >
                    {googleWaiting ? <Spinner size={16} /> : (
                        <svg width="18" height="18" viewBox="0 0 48 48" aria-hidden="true">
                            <path fill="#FFC107" d="M43.6 20.5H42V20H24v8h11.3C33.7 32.7 29.2 36 24 36c-6.6 0-12-5.4-12-12s5.4-12 12-12c3 0 5.8 1.1 7.9 3l5.7-5.7C34 6.1 29.3 4 24 4 12.9 4 4 12.9 4 24s8.9 20 20 20 20-8.9 20-20c0-1.3-.1-2.4-.4-3.5z" />
                            <path fill="#FF3D00" d="M6.3 14.7l6.6 4.8C14.7 15.1 19 12 24 12c3 0 5.8 1.1 7.9 3l5.7-5.7C34 6.1 29.3 4 24 4 16.3 4 9.7 8.3 6.3 14.7z" />
                            <path fill="#4CAF50" d="M24 44c5.2 0 9.9-2 13.4-5.2l-6.2-5.2C29.2 35.1 26.7 36 24 36c-5.2 0-9.6-3.3-11.3-8l-6.5 5C9.5 39.6 16.2 44 24 44z" />
                            <path fill="#1976D2" d="M43.6 20.5H42V20H24v8h11.3c-.8 2.2-2.2 4.2-4.1 5.6l6.2 5.2C37 38.2 44 33 44 24c0-1.3-.1-2.4-.4-3.5z" />
                        </svg>
                    )}
                    {googleWaiting ? 'Aguardando o navegador…' : registering ? 'Criar conta com Google' : 'Entrar com Google'}
                </button>

                <div className="divider-or label-mono my-4">ou</div>

                <form onSubmit={submit} noValidate>
                    {registering && (
                        <label className="mb-3.5 block">
                            <span className="label-mono mb-2 block">Apelido</span>
                            <input
                                className="field w-full"
                                type="text"
                                maxLength={32}
                                value={name}
                                onChange={event => setName(event.target.value.replace(/\s+/g, ''))}
                                placeholder="edsu"
                                title="Letras, números, ponto e _ — sem espaço"
                                autoComplete="username"
                                autoCapitalize="off"
                                autoCorrect="off"
                                spellCheck={false}
                            />
                            <span className="mt-1 block text-[11px] text-ink-dim">É como as pessoas te acham. Sem espaço, e ninguém mais pode usar o mesmo.</span>
                        </label>
                    )}
                    <label className="block">
                        <span className="label-mono mb-2 block">E-mail</span>
                        <input className="field w-full" type="email" value={email} onChange={event => setEmail(event.target.value)} placeholder="voce@email.com" autoComplete="username" />
                    </label>
                    <label className="mt-3.5 block">
                        <span className="label-mono mb-2 block">Senha</span>
                        <input className="field w-full" type="password" value={password} onChange={event => setPassword(event.target.value)} placeholder={registering ? '8 ou mais' : '••••••••'} autoComplete={registering ? 'new-password' : 'current-password'} />
                    </label>
                    <button className="btn-primary mt-4 flex w-full items-center justify-center gap-2 text-[14.5px]" type="submit" disabled={loginBusy && ! googleWaiting}>
                        {loginBusy && ! googleWaiting && <Spinner size={15} />}
                        {registering ? 'Criar conta' : 'Entrar'}
                    </button>
                </form>

                <p className="mt-3 min-h-5 text-center text-[12.5px] text-danger" role="alert">{loginError}</p>

                <p className="mt-4 text-center text-[12.5px] text-ink-dim">
                    {registering ? 'Já tem conta? ' : 'Não tem conta? '}
                    <button className="cursor-pointer text-lilac underline" type="button" onClick={() => hub.setLoginMode(registering ? 'login' : 'register')}>
                        {registering ? 'Entrar' : 'Criar conta'}
                    </button>
                </p>
            </div>
        </div>
    );
}
