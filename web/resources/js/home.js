import { gsap } from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import * as THREE from 'three';

class HomePage {
    constructor(canvas) {
        this.canvas = canvas;
        this.dead = false;
        this.reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    }

    start() {
        gsap.registerPlugin(ScrollTrigger);
        this.reveals();
        this.parallax();
        this.void();
    }

    stop() {
        this.dead = true;
        cancelAnimationFrame(this.raf);
        window.removeEventListener('resize', this.onResize);
        window.removeEventListener('pointermove', this.onMove);
        ScrollTrigger.getAll().forEach((trigger) => trigger.kill());
    }

    reveals() {
        if (this.reduce) {
            return;
        }

        const height = window.innerHeight || 800;
        let above = 0;

        document.querySelectorAll('[data-reveal]').forEach((element) => {
            const options = { opacity: 0, y: 30, scale: 0.985, duration: 0.8, ease: 'power3.out', clearProps: 'opacity,transform' };

            if (element.getBoundingClientRect().top < height - 20) {
                options.delay = above * 0.08;
                above += 1;
            } else {
                options.scrollTrigger = { trigger: element, start: 'top 92%', once: true };
            }

            gsap.from(element, options);
        });
    }

    parallax() {
        if (this.reduce) {
            return;
        }

        document.querySelectorAll('[data-parallax]').forEach((element) => {
            const amount = parseFloat(element.getAttribute('data-parallax')) || 40;

            gsap.fromTo(element, { y: amount }, {
                y: -amount,
                ease: 'none',
                scrollTrigger: { trigger: element, start: 'top bottom', end: 'bottom top', scrub: 0.6 },
            });
        });
    }

    void() {
        const renderer = new THREE.WebGLRenderer({ canvas: this.canvas, alpha: true, antialias: true });

        renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));

        const scene = new THREE.Scene();
        const camera = new THREE.PerspectiveCamera(60, 1, 0.1, 200);

        camera.position.z = 18;

        const layers = this.layers();

        layers.forEach((points) => scene.add(points));

        this.onResize = () => {
            renderer.setSize(window.innerWidth, window.innerHeight, false);
            camera.aspect = window.innerWidth / window.innerHeight;
            camera.updateProjectionMatrix();
        };
        this.onResize();
        window.addEventListener('resize', this.onResize);

        let mouseX = 0;
        let mouseY = 0;
        let targetX = 0;
        let targetY = 0;
        let scroll = 0;

        this.onMove = (event) => {
            targetX = (event.clientX / window.innerWidth - 0.5) * 2;
            targetY = (event.clientY / window.innerHeight - 0.5) * 2;
        };
        window.addEventListener('pointermove', this.onMove);

        const started = performance.now();
        const speed = this.reduce ? 0 : 1;

        const loop = () => {
            if (this.dead) {
                return;
            }

            const elapsed = (performance.now() - started) / 1000;
            const root = document.scrollingElement || document.documentElement;
            const max = Math.max(1, root.scrollHeight - window.innerHeight);

            scroll += ((root.scrollTop || window.scrollY || 0) / max - scroll) * 0.08;
            mouseX += (targetX - mouseX) * 0.045;
            mouseY += (targetY - mouseY) * 0.045;

            layers.forEach((points) => {
                const depth = points.userData.depth;

                points.position.y = scroll * 26 * depth;
                points.rotation.y = elapsed * 0.02 * depth * speed + mouseX * 0.18 * depth;
                points.rotation.x = mouseY * 0.1 * depth;
            });

            camera.position.x = mouseX * 0.8;
            camera.position.y = -mouseY * 0.45;
            camera.lookAt(0, scroll * 2, 0);
            renderer.render(scene, camera);
            this.raf = requestAnimationFrame(loop);
        };

        loop();
    }

    layers() {
        const dot = document.createElement('canvas');

        dot.width = 64;
        dot.height = 64;

        const context = dot.getContext('2d');
        const gradient = context.createRadialGradient(32, 32, 0, 32, 32, 32);

        gradient.addColorStop(0, 'rgba(255,255,255,1)');
        gradient.addColorStop(0.45, 'rgba(255,255,255,0.9)');
        gradient.addColorStop(1, 'rgba(255,255,255,0)');
        context.fillStyle = gradient;
        context.beginPath();
        context.arc(32, 32, 32, 0, Math.PI * 2);
        context.fill();

        const texture = new THREE.CanvasTexture(dot);
        const violet = new THREE.Color('#8a7cf5');
        const pale = new THREE.Color('#dcdcf2');
        const specs = window.innerWidth < 700
            ? [{ n: 500, r: 26, s: 0.06, d: 0.35 }, { n: 320, r: 16, s: 0.09, d: 0.8 }]
            : [{ n: 1200, r: 34, s: 0.055, d: 0.3 }, { n: 700, r: 22, s: 0.085, d: 0.7 }, { n: 260, r: 13, s: 0.13, d: 1.2 }];

        return specs.map((spec) => {
            const positions = new Float32Array(spec.n * 3);
            const colors = new Float32Array(spec.n * 3);

            for (let index = 0; index < spec.n; index += 1) {
                positions[index * 3] = (Math.random() - 0.5) * spec.r * 2.4;
                positions[index * 3 + 1] = (Math.random() - 0.5) * spec.r * 2.6;
                positions[index * 3 + 2] = (Math.random() - 0.5) * spec.r;

                const color = Math.random() < 0.45 ? violet : pale;

                colors[index * 3] = color.r;
                colors[index * 3 + 1] = color.g;
                colors[index * 3 + 2] = color.b;
            }

            const geometry = new THREE.BufferGeometry();

            geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
            geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));

            const points = new THREE.Points(geometry, new THREE.PointsMaterial({
                size: spec.s * 1.6,
                vertexColors: true,
                transparent: true,
                opacity: 0.85,
                map: texture,
                alphaTest: 0.01,
                sizeAttenuation: true,
                depthWrite: false,
                blending: THREE.AdditiveBlending,
            }));

            points.userData.depth = spec.d;

            return points;
        });
    }
}

const SCREENS = ['entrada', 'sala', 'share'];

class Landing {
    constructor(links, aptCommands) {
        this.links = links;
        this.aptCommands = aptCommands;
        this.os = this.detect();
        this.screen = SCREENS.includes(new URLSearchParams(location.search).get('tela')) ? new URLSearchParams(location.search).get('tela') : 'entrada';
        this.copied = false;
    }

    select(id) {
        const direction = SCREENS.indexOf(id) > SCREENS.indexOf(this.screen) ? 1 : -1;

        if (id === this.screen) {
            return;
        }

        this.screen = id;

        const pane = document.querySelector('[data-pane]');

        if (pane && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
            gsap.fromTo(pane, { x: direction * 70, opacity: 0 }, { x: 0, opacity: 1, duration: 0.45, ease: 'power3.out', clearProps: 'transform,opacity' });
        }
    }

    detect() {
        const agent = navigator.userAgent || '';

        if (/Windows|Win32|Win64/i.test(agent)) {
            return 'Windows';
        }

        if (/Linux|X11|CrOS/i.test(agent)) {
            return 'Linux';
        }

        return 'macOS';
    }

    get primaryLabel() {
        return `Baixar para ${this.os}`;
    }

    get primaryHref() {
        return this.links[this.os] ?? '#download';
    }

    get others() {
        return ['macOS', 'Windows', 'Linux'].filter((platform) => platform !== this.os);
    }

    copyApt() {
        const done = () => {
            this.copied = true;
            clearTimeout(this.timer);
            this.timer = setTimeout(() => { this.copied = false; }, 1600);
        };

        navigator.clipboard?.writeText(this.aptCommands).then(done, done) ?? done();
    }
}

document.addEventListener('alpine:init', () => {
    window.Alpine.data('landing', (links, aptCommands) => new Landing(links, aptCommands));
});

const canvas = document.querySelector('[data-void]');

if (canvas) {
    new HomePage(canvas).start();
}
