import './echo';

import { ChatSocket } from './voice/ChatSocket.js';
import { VoiceStage } from './voice/VoiceStage.js';

const voice = new VoiceStage();
const chat = new ChatSocket();

window.voice = voice;
voice.start();
chat.start();
