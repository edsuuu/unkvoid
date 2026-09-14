import { App } from './app.js';

const app = new App();

void app.start();

// Exposto de propósito: a janela do Tauri não tem console, e é por aqui que dá para
// cutucar o estado do app pelo harness.
window.unkvoid = app;
