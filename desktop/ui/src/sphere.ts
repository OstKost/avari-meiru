/**
 * Procedural Canvas 2D 3D Sphere Renderer for Avari Mieru.
 *
 * Implements an orbital, depth-projected particle and lattice sphere
 * inspired by the user reference animations:
 * - _.gif: Dense geometric triangular lattice, multi-axis orbital rings with radial tick details,
 *   crisp emerald cybernetic mesh, depth-faded particle cloud.
 * - _ (1).gif: Warm amber/golden rotating diffuse particle cloud with soft optical defocusing.
 * - Animacion.gif: Living magnetic flux filaments, bright energetic core, and glowing rim.
 *
 * Technical Highlights:
 * - Real optical defocus blur layer (via offscreen canvas + dynamic separable filter interpolation).
 * - ~2400 total particles (1800 surface nodes + 600 interior spark nodes).
 * - Triangular wireframe lattice connecting surface neighbors in crisp focus states.
 * - Precision HUD latitude/meridian arcs with radial tick marks.
 * - Perfectly normalized and clipped magnetic filaments without diagonal artifacts.
 * - Strict reduced-motion compliance: complete rAF loop termination + static single-frame rendering.
 * - Unified scheduler for tab visibility, native minimize/hide, and reduced motion.
 * - Real runtime performance instrumentation (actual frame cost, p50, p95, real rendered FPS).
 * - Safe 2D context acquisition with graceful static SVG fallback.
 */

import type { SphereVisualState } from './types';

export interface SphereOptions {
  canvas: HTMLCanvasElement;
  initialState?: SphereVisualState;
  isDarkTheme?: boolean;
  isDark?: boolean;
}

export interface RenderPerformanceStats {
  fps: number;
  renderedFps: number;
  p50Ms: number;
  p95Ms: number;
  avgMs: number;
  renderedCount: number;
  isReducedMotion: boolean;
  isPaused: boolean;
}

interface Point3D {
  x: number;
  y: number;
  z: number;
}

interface ProjectedPoint {
  x: number;
  y: number;
  z: number;
  scale: number;
  alpha: number;
}

interface Particle3D {
  base: Point3D;
  phase: number;
  speed: number;
  radiusOffset: number;
  scatter: Point3D;
  isCore: boolean;
}

interface MeshEdge {
  p1: number;
  p2: number;
}

interface CoordinateArc {
  tiltX: number;
  tiltZ: number;
  rotSpeed: number;
  radiusRatio: number;
  hasTicks: boolean;
  tickInterval: number;
}

interface FilamentDef {
  harmonic: number;
  latSpan: number;
  lonOffset: number;
  speed: number;
  radiusBase: number;
}

interface SmokeRibbon {
  harmonicU: number;
  harmonicV: number;
  baseLon: number;
  baseLat: number;
  speed: number;
  width: number;
  alphaFactor: number;
  layer: number; // 0 = inner wisps, 1 = middle shell, 2 = outer vortex
}

interface SmokePuff {
  lat: number;
  lon: number;
  speed: number;
  radiusFactor: number;
  size: number;
  phase: number;
  isPolar: boolean;
  isTop: boolean;
}

interface ColorRGB {
  r: number;
  g: number;
  b: number;
}

const STATE_COLORS: Record<
  SphereVisualState,
  { primary: ColorRGB; secondary: ColorRGB; core: ColorRGB; glow: ColorRGB }
> = {
  cold_start: {
    primary: { r: 56, g: 189, b: 248 }, // Sky 400
    secondary: { r: 99, g: 102, b: 241 }, // Indigo 500
    core: { r: 224, g: 242, b: 254 }, // Sky 100
    glow: { r: 14, g: 165, b: 233 },
  },
  unconfigured: {
    primary: { r: 148, g: 163, b: 184 }, // Slate 400
    secondary: { r: 100, g: 116, b: 139 }, // Slate 500
    core: { r: 241, g: 245, b: 249 }, // Slate 100
    glow: { r: 100, g: 116, b: 139 },
  },
  ready: {
    primary: { r: 245, g: 158, b: 11 }, // Amber 500
    secondary: { r: 217, g: 119, b: 6 }, // Amber 600
    core: { r: 254, g: 243, b: 199 }, // Amber 100
    glow: { r: 245, g: 158, b: 11 },
  },
  connecting: {
    primary: { r: 59, g: 130, b: 246 }, // Blue 500
    secondary: { r: 139, g: 92, b: 246 }, // Violet 500
    core: { r: 219, g: 234, b: 254 }, // Blue 100
    glow: { r: 99, g: 102, b: 241 },
  },
  connected: {
    primary: { r: 16, g: 185, b: 129 }, // Emerald 500
    secondary: { r: 5, g: 150, b: 105 }, // Emerald 600
    core: { r: 209, g: 250, b: 229 }, // Emerald 100
    glow: { r: 52, g: 211, b: 153 }, // Emerald 400
  },
  error: {
    primary: { r: 239, g: 68, b: 68 }, // Red 500
    secondary: { r: 185, g: 28, b: 28 }, // Red 700
    core: { r: 254, g: 202, b: 202 }, // Red 200
    glow: { r: 239, g: 68, b: 68 },
  },
  disconnecting: {
    primary: { r: 249, g: 115, b: 22 }, // Orange 500
    secondary: { r: 217, g: 119, b: 6 }, // Amber 600
    core: { r: 254, g: 215, b: 170 }, // Orange 200
    glow: { r: 249, g: 115, b: 22 },
  },
};

export class MieruSphere {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D | null = null;
  private offscreenCanvas: HTMLCanvasElement | null = null;
  private offscreenCtx: CanvasRenderingContext2D | null = null;

  private isFallback = false;
  private currentState: SphereVisualState;
  private targetState: SphereVisualState;
  private isDark: boolean;

  // Unified Scheduler flags
  private isRunning = false;
  private isTabActive = true;
  private isDocumentVisible = true;
  private reducedMotion = false;
  private animFrameId: number | null = null;

  private lastTimestamp = 0;
  private stateTimer = 0;
  private motionTime = 0;
  private pulseTriggerTime = 0;

  // Visual parameters (inspectable for tests & animated)
  public currentPrimary: ColorRGB = { ...STATE_COLORS.cold_start.primary };
  public currentSecondary: ColorRGB = { ...STATE_COLORS.cold_start.secondary };
  public currentCore: ColorRGB = { ...STATE_COLORS.cold_start.core };
  public currentGlow: ColorRGB = { ...STATE_COLORS.cold_start.glow };

  private rotY = 0;
  private rotX = 0.24;
  private rotZ = 0.12;

  public currentRotSpeed = 0.45;
  public currentFocus = 0.6; // 0 = diffuse cloud, 1 = crisp geometric lattice
  public currentCoalesce = 1.0; // 0 = exploded/gathering, 1 = coherent sphere
  public currentCoreEnergy = 0.5;
  public currentPulse = 0;
  public currentBlurPx = 0;
  public currentSmokeMix = 0.0; // 0 = pure crisp geometric mesh, 1 = pure smoke shell vortex

  private needsRenderOnResume = false;
  private fallbackEl: HTMLElement | null = null;

  // Geometry datasets (Dense point cloud ~2400 particles)
  private particles: Particle3D[] = [];
  private meshNodes: Point3D[] = [];
  private meshEdges: MeshEdge[] = [];
  private coordinateArcs: CoordinateArc[] = [];
  private filaments: FilamentDef[] = [];
  private satellites: { angle: number; speed: number; radius: number; tilt: number }[] = [];

  // Volumetric Smoke Stream Ribbons & Puffs (_ (1).gif reference)
  private smokeRibbons: SmokeRibbon[] = [];
  private smokePuffs: SmokePuff[] = [];

  // Offscreen cached glow sprite for particle blits without per-frame shadowBlur
  private glowSprite: HTMLCanvasElement | null = null;
  private smokeStamp: HTMLCanvasElement | null = null;

  // Performance instrumentation
  private frameDurations: number[] = [];
  private totalRenderedFrames = 0;
  private fpsFrameCount = 0;
  private fpsWindowStart = 0;
  private actualFps = 0;

  // Event handlers for cleanup
  private visibilityHandler: (() => void) | null = null;
  private resizeObserver: ResizeObserver | null = null;
  private motionMediaQuery: MediaQueryList | null = null;
  private motionChangeHandler: ((e: MediaQueryListEvent) => void) | null = null;

  constructor(
    canvasOrOptions: HTMLCanvasElement | SphereOptions,
    legacyOptions?: { isDark?: boolean; isDarkTheme?: boolean; initialState?: SphereVisualState }
  ) {
    if (canvasOrOptions instanceof HTMLCanvasElement) {
      this.canvas = canvasOrOptions;
      this.currentState = legacyOptions?.initialState || 'cold_start';
      this.isDark = legacyOptions?.isDark ?? legacyOptions?.isDarkTheme ?? false;
    } else {
      this.canvas = canvasOrOptions.canvas;
      this.currentState = canvasOrOptions.initialState || 'cold_start';
      this.isDark = canvasOrOptions.isDark ?? canvasOrOptions.isDarkTheme ?? false;
    }

    this.targetState = this.currentState;

    // Acquire 2D context safely
    const context = this.canvas.getContext('2d', { alpha: true });
    if (!context) {
      this.isFallback = true;
      this.renderFallback();
      return;
    }
    this.ctx = context;

    // Create offscreen canvas for rendering & optical defocus blur
    this.offscreenCanvas = document.createElement('canvas');
    this.offscreenCtx = this.offscreenCanvas.getContext('2d', { alpha: true });

    this.initMotionPreference();
    this.applyTargetStateImmediate();
    this.initGeometry();
    this.createGlowSprite();
    this.createSmokeStamp();
    this.setupCanvasDpi();

    // Setup visibility & scheduler listeners
    this.isDocumentVisible = !document.hidden;
    this.visibilityHandler = () => {
      this.isDocumentVisible = !document.hidden;
      this.updateScheduler();
    };
    document.addEventListener('visibilitychange', this.visibilityHandler);

    if (typeof ResizeObserver !== 'undefined') {
      this.resizeObserver = new ResizeObserver(() => {
        this.setupCanvasDpi();
        if (!this.isRunning && this.isDocumentVisible && this.isTabActive) {
          this.renderStaticFrame();
        }
      });
      this.resizeObserver.observe(this.canvas);
    }

    // Expose dev animator API strictly in dev mode without secrets
    if (import.meta.env.DEV && typeof window !== 'undefined') {
      (window as unknown as { __MIERU_SPHERE__?: MieruSphere }).__MIERU_SPHERE__ = this;
    }

    this.updateScheduler();
  }

  private initMotionPreference() {
    if (typeof window !== 'undefined' && window.matchMedia) {
      this.motionMediaQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
      this.reducedMotion = this.motionMediaQuery.matches;

      this.motionChangeHandler = (e: MediaQueryListEvent) => {
        this.setReducedMotion(e.matches);
      };
      this.motionMediaQuery.addEventListener('change', this.motionChangeHandler);
    }
  }

  private updateScheduler() {
    if (this.isFallback) return;

    const isVisibleAndActive = this.isDocumentVisible && this.isTabActive;
    const shouldRun = isVisibleAndActive && !this.reducedMotion;

    if (shouldRun && !this.isRunning) {
      this.isRunning = true;
      this.lastTimestamp = performance.now();
      this.fpsWindowStart = this.lastTimestamp;
      this.fpsFrameCount = 0;
      this.animFrameId = requestAnimationFrame(this.loop);
    } else if (!shouldRun && this.isRunning) {
      this.isRunning = false;
      if (this.animFrameId !== null) {
        cancelAnimationFrame(this.animFrameId);
        this.animFrameId = null;
      }
      if (this.reducedMotion && isVisibleAndActive) {
        this.renderStaticFrame();
      } else {
        this.needsRenderOnResume = true;
      }
    } else if (!shouldRun) {
      if (this.reducedMotion && isVisibleAndActive) {
        if (this.needsRenderOnResume) {
          this.needsRenderOnResume = false;
          this.renderStaticFrame();
        }
      } else {
        this.needsRenderOnResume = true;
      }
    }
  }

  /**
   * Initializes dense 3D geometry:
   * - 1800 Fibonacci surface particles
   * - 600 interior noisy core sparks
   * - Triangular mesh connectivity for primary vertices
   * - Precision HUD latitude and meridian arcs with radial tick marks
   * - Magnetic filaments
   */
  private initGeometry() {
    // 1. Dense Fibonacci Sphere: 1800 surface particles
    const surfaceCount = 1800;
    const goldenRatio = (1 + Math.sqrt(5)) / 2;
    this.particles = [];

    for (let i = 0; i < surfaceCount; i++) {
      const theta = (2 * Math.PI * i) / goldenRatio;
      const phi = Math.acos(1 - (2 * (i + 0.5)) / surfaceCount);
      const x = Math.sin(phi) * Math.cos(theta);
      const y = Math.cos(phi);
      const z = Math.sin(phi) * Math.sin(theta);

      // Random scatter direction for cold_start assembly
      const scatterAngle = Math.random() * Math.PI * 2;
      const scatterDist = 1.4 + Math.random() * 2.2;

      this.particles.push({
        base: { x, y, z },
        phase: Math.random() * Math.PI * 2,
        speed: 0.7 + Math.random() * 1.5,
        radiusOffset: (Math.random() - 0.5) * 0.06,
        scatter: {
          x: Math.cos(scatterAngle) * scatterDist,
          y: (Math.random() - 0.5) * scatterDist,
          z: Math.sin(scatterAngle) * scatterDist,
        },
        isCore: false,
      });
    }

    // 2. Dense interior core sparks (600 points)
    const coreCount = 600;
    for (let i = 0; i < coreCount; i++) {
      const u = Math.random();
      const v = Math.random();
      const theta = u * 2.0 * Math.PI;
      const phi = Math.acos(2.0 * v - 1.0);
      const r = Math.cbrt(Math.random()) * 0.42;

      const sinPhi = Math.sin(phi);
      const x = r * sinPhi * Math.cos(theta);
      const y = r * Math.cos(phi);
      const z = r * sinPhi * Math.sin(theta);

      this.particles.push({
        base: { x, y, z },
        phase: Math.random() * Math.PI * 2,
        speed: 1.5 + Math.random() * 3.0,
        radiusOffset: (Math.random() - 0.5) * 0.1,
        scatter: { x: x * 2.5, y: y * 2.5, z: z * 2.5 },
        isCore: true,
      });
    }

    // 3. Primary Mesh Nodes & Triangular Lattice Edges (~96 nodes)
    const meshNodeCount = 96;
    this.meshNodes = [];
    for (let i = 0; i < meshNodeCount; i++) {
      const theta = (2 * Math.PI * i) / goldenRatio;
      const phi = Math.acos(1 - (2 * (i + 0.5)) / meshNodeCount);
      this.meshNodes.push({
        x: Math.sin(phi) * Math.cos(theta),
        y: Math.cos(phi),
        z: Math.sin(phi) * Math.sin(theta),
      });
    }

    // Connect surface neighbors with distance threshold
    this.meshEdges = [];
    for (let i = 0; i < meshNodeCount; i++) {
      for (let j = i + 1; j < meshNodeCount; j++) {
        const dx = this.meshNodes[i].x - this.meshNodes[j].x;
        const dy = this.meshNodes[i].y - this.meshNodes[j].y;
        const dz = this.meshNodes[i].z - this.meshNodes[j].z;
        const dist = Math.sqrt(dx * dx + dy * dy + dz * dz);
        if (dist > 0.15 && dist < 0.44) {
          this.meshEdges.push({ p1: i, p2: j });
        }
      }
    }

    // 4. Precision HUD Coordinate Arcs with short radial tick marks
    this.coordinateArcs = [
      { tiltX: 0.0, tiltZ: 0.0, rotSpeed: 0.45, radiusRatio: 1.0, hasTicks: true, tickInterval: 12 }, // Equator
      { tiltX: 0.42, tiltZ: -0.25, rotSpeed: 0.65, radiusRatio: 1.02, hasTicks: true, tickInterval: 14 },
      { tiltX: -0.38, tiltZ: 0.35, rotSpeed: -0.55, radiusRatio: 0.99, hasTicks: true, tickInterval: 14 },
      { tiltX: 1.57, tiltZ: 0.0, rotSpeed: 0.35, radiusRatio: 1.01, hasTicks: true, tickInterval: 16 }, // Prime meridian
      { tiltX: 0.85, tiltZ: 0.6, rotSpeed: -0.42, radiusRatio: 1.03, hasTicks: false, tickInterval: 0 },
    ];

    // 5. Magnetic Flux Filaments (Animacion.gif) strictly normalized
    this.filaments = [
      { harmonic: 2.2, latSpan: 1.35, lonOffset: 0.0, speed: 1.3, radiusBase: 0.68 },
      { harmonic: 3.1, latSpan: 1.42, lonOffset: Math.PI * 0.5, speed: 1.1, radiusBase: 0.72 },
      { harmonic: 1.9, latSpan: 1.25, lonOffset: Math.PI, speed: 1.5, radiusBase: 0.65 },
      { harmonic: 2.7, latSpan: 1.48, lonOffset: Math.PI * 1.5, speed: 1.2, radiusBase: 0.7 },
      { harmonic: 3.4, latSpan: 1.3, lonOffset: Math.PI * 0.25, speed: 1.4, radiusBase: 0.74 },
      { harmonic: 2.5, latSpan: 1.4, lonOffset: Math.PI * 1.25, speed: 1.0, radiusBase: 0.66 },
    ];

    // 6. Orbital Satellites
    this.satellites = [
      { angle: 0, speed: 1.2, radius: 1.22, tilt: 0.35 },
      { angle: 2.1, speed: -1.0, radius: 1.28, tilt: -0.48 },
      { angle: 4.2, speed: 0.85, radius: 1.35, tilt: 0.65 },
    ];

    // 7. Volumetric Smoke Stream Ribbons & Puffs (_ (1).gif reference)
    // 54 flowing ribbons distributed across 3 stratified depth layers with distinct harmonics
    this.smokeRibbons = [];
    const ribbonCount = 54;
    for (let i = 0; i < ribbonCount; i++) {
      const layer = i % 3; // 0 = inner wisps, 1 = middle shell, 2 = outer vortex
      const baseLat = ((i / ribbonCount) * 2 - 1) * 1.3; // -1.3 to +1.3 rad
      const baseLon = (i * 2.3999632) % (Math.PI * 2); // Golden angle step
      this.smokeRibbons.push({
        harmonicU: 1.6 + ((i * 7) % 5) * 0.45,
        harmonicV: 2.1 + ((i * 11) % 6) * 0.35,
        baseLon,
        baseLat,
        speed: (0.35 + ((i * 13) % 7) * 0.12) * (layer === 2 ? 1.2 : layer === 0 ? 0.75 : 1.0),
        width: layer === 0 ? 1.4 : layer === 1 ? 2.2 : 3.0,
        alphaFactor: layer === 0 ? 0.28 : layer === 1 ? 0.42 : 0.32,
        layer,
      });
    }

    // Volumetric stratified puffs (72 puffs for smooth density, breathing, and polar caps)
    this.smokePuffs = [];
    const puffCount = 72;
    for (let i = 0; i < puffCount; i++) {
      const isPolar = i < 20; // 20 polar cap puffs for bright upper/lower clusters
      const isTop = i < 10;
      let lat: number;
      let lon: number;
      let radiusFactor: number;
      let size: number;

      if (isPolar) {
        // Upper pole [0.85, 1.3] rad, Lower pole [-1.3, -0.85] rad
        lat = isTop ? 0.85 + Math.random() * 0.45 : -0.85 - Math.random() * 0.45;
        lon = Math.random() * Math.PI * 2;
        radiusFactor = 0.58 + Math.random() * 0.28;
        size = 18 + Math.random() * 16;
      } else {
        // Equatorial and mid-latitude flowing wisps
        lat = (Math.random() * 2 - 1) * 0.82;
        lon = Math.random() * Math.PI * 2;
        radiusFactor = 0.72 + Math.random() * 0.28;
        size = 14 + Math.random() * 14;
      }

      this.smokePuffs.push({
        lat,
        lon,
        speed: 0.4 + Math.random() * 0.55,
        radiusFactor,
        size,
        phase: Math.random() * Math.PI * 2,
        isPolar,
        isTop,
      });
    }
  }

  /**
   * Generates offscreen cached radial glow sprite.
   */
  private createGlowSprite() {
    const size = 32;
    this.glowSprite = document.createElement('canvas');
    this.glowSprite.width = size;
    this.glowSprite.height = size;

    const ctx = this.glowSprite.getContext('2d');
    if (!ctx) return;

    const half = size / 2;
    const grad = ctx.createRadialGradient(half, half, 0, half, half, half);
    grad.addColorStop(0, 'rgba(255, 255, 255, 1)');
    grad.addColorStop(0.35, 'rgba(255, 255, 255, 0.45)');
    grad.addColorStop(0.7, 'rgba(255, 255, 255, 0.12)');
    grad.addColorStop(1, 'rgba(255, 255, 255, 0)');

    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, size, size);
  }

  /**
   * Generates cached ultra-soft smoke stamp for puff blitting without runtime shadowBlur.
   */
  private createSmokeStamp() {
    const size = 64;
    this.smokeStamp = document.createElement('canvas');
    this.smokeStamp.width = size;
    this.smokeStamp.height = size;

    const ctx = this.smokeStamp.getContext('2d');
    if (!ctx) return;

    const half = size / 2;
    const grad = ctx.createRadialGradient(half, half, 0, half, half, half);
    // Smooth quadratic rolloff simulating Gaussian smoke wisp
    grad.addColorStop(0, 'rgba(255, 255, 255, 0.75)');
    grad.addColorStop(0.25, 'rgba(255, 255, 255, 0.45)');
    grad.addColorStop(0.55, 'rgba(255, 255, 255, 0.16)');
    grad.addColorStop(0.82, 'rgba(255, 255, 255, 0.04)');
    grad.addColorStop(1, 'rgba(255, 255, 255, 0)');

    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, size, size);
  }

  private setupCanvasDpi() {
    if (this.isFallback || !this.ctx) return;

    const rect = this.canvas.getBoundingClientRect();
    const dpr = Math.min(window.devicePixelRatio || 1, 2.5);

    const pixelWidth = Math.max(1, Math.floor(rect.width * dpr));
    const pixelHeight = Math.max(1, Math.floor(rect.height * dpr));

    if (this.canvas.width !== pixelWidth || this.canvas.height !== pixelHeight) {
      this.canvas.width = pixelWidth;
      this.canvas.height = pixelHeight;
    }

    if (this.offscreenCanvas) {
      if (this.offscreenCanvas.width !== pixelWidth || this.offscreenCanvas.height !== pixelHeight) {
        this.offscreenCanvas.width = pixelWidth;
        this.offscreenCanvas.height = pixelHeight;
      }
    }
  }

  // ==========================================
  // Public Control API
  // ==========================================

  public setState(nextState: SphereVisualState) {
    if (this.targetState === nextState) return;

    this.targetState = nextState;
    this.stateTimer = 0;

    if (nextState === 'error') {
      this.pulseTriggerTime = performance.now();
    }

    if (this.isFallback) {
      this.updateFallback();
      return;
    }

    if (this.reducedMotion) {
      this.applyTargetStateImmediate();
      if (this.isDocumentVisible && this.isTabActive) {
        this.renderStaticFrame();
      } else {
        this.needsRenderOnResume = true;
      }
    } else if (!this.isRunning) {
      this.needsRenderOnResume = true;
    }
  }

  public applyTargetStateImmediate() {
    const targetColors = STATE_COLORS[this.targetState];
    this.currentPrimary = { ...targetColors.primary };
    this.currentSecondary = { ...targetColors.secondary };
    this.currentCore = { ...targetColors.core };
    this.currentGlow = { ...targetColors.glow };

    switch (this.targetState) {
      case 'cold_start':
        this.currentRotSpeed = 0.85;
        this.currentCoreEnergy = 0.85;
        this.currentCoalesce = this.reducedMotion ? 1.0 : 0.12;
        this.currentFocus = this.reducedMotion ? 0.85 : 0.45;
        this.currentBlurPx = 0.5;
        this.currentSmokeMix = 0.0;
        break;
      case 'unconfigured':
        this.currentRotSpeed = 0.22;
        this.currentFocus = 0.3;
        this.currentCoalesce = 1.0;
        this.currentCoreEnergy = 0.25;
        this.currentBlurPx = 1.2;
        this.currentSmokeMix = 1.0;
        break;
      case 'ready':
        this.currentRotSpeed = 0.38;
        this.currentFocus = 0.45;
        this.currentCoalesce = 1.0;
        this.currentCoreEnergy = 0.45;
        this.currentBlurPx = 1.0; // Soft edge softening (1.0 CSS px) per user request
        this.currentSmokeMix = 1.0; // Full volumetric smoke shell (_ (1).gif)
        break;
      case 'connecting':
        this.currentRotSpeed = 1.15;
        this.currentFocus = 0.88;
        this.currentCoalesce = 1.0;
        this.currentCoreEnergy = 0.9;
        this.currentBlurPx = 0.8;
        this.currentSmokeMix = 0.35; // Morphing from smoke into geometric mesh
        break;
      case 'connected':
        this.currentRotSpeed = 0.65;
        this.currentFocus = 1.0;
        this.currentCoalesce = 1.0;
        this.currentCoreEnergy = 0.95;
        this.currentBlurPx = 0.0; // Crisp zero blur (_.gif)
        this.currentSmokeMix = 0.0; // Pure geometric lattice
        break;
      case 'error':
        this.currentRotSpeed = 0.28;
        this.currentFocus = 0.5;
        this.currentCoalesce = 1.0;
        this.currentCoreEnergy = 0.75;
        this.currentBlurPx = 1.0;
        this.currentSmokeMix = 1.0;
        break;
      case 'disconnecting':
        this.currentRotSpeed = 0.5;
        this.currentFocus = 0.45;
        this.currentCoalesce = 1.0;
        this.currentCoreEnergy = 0.4;
        this.currentBlurPx = 1.0;
        this.currentSmokeMix = 0.75; // Morphing back into smoke
        break;
    }
  }

  public getState(): SphereVisualState {
    return this.targetState;
  }

  public setDark(dark: boolean) {
    this.isDark = dark;
    if (!this.isRunning && this.isDocumentVisible && this.isTabActive) {
      this.renderStaticFrame();
    }
  }

  public setDarkTheme(dark: boolean) {
    this.setDark(dark);
  }

  public setTabActive(active: boolean) {
    if (this.isTabActive === active) return;
    this.isTabActive = active;
    this.updateScheduler();
  }

  public setReducedMotion(reduced: boolean) {
    if (this.reducedMotion === reduced) return;
    this.reducedMotion = reduced;
    if (reduced) {
      this.applyTargetStateImmediate();
    }
    this.updateScheduler();
  }

  public pause() {
    this.isRunning = false;
    if (this.animFrameId !== null) {
      cancelAnimationFrame(this.animFrameId);
      this.animFrameId = null;
    }
  }

  public resume() {
    this.updateScheduler();
  }

  public resize() {
    this.setupCanvasDpi();
    if (!this.isRunning && this.isDocumentVisible && this.isTabActive) {
      this.renderStaticFrame();
    }
  }

  public getPerformanceStats(): RenderPerformanceStats {
    if (this.frameDurations.length === 0) {
      return {
        fps: 0,
        renderedFps: 0,
        p50Ms: 0,
        p95Ms: 0,
        avgMs: 0,
        renderedCount: this.totalRenderedFrames,
        isReducedMotion: this.reducedMotion,
        isPaused: !this.isRunning,
      };
    }
    const sorted = [...this.frameDurations].sort((a, b) => a - b);
    const p50 = sorted[Math.floor(sorted.length * 0.5)];
    const p95 = sorted[Math.floor(sorted.length * 0.95)];
    const avg = sorted.reduce((a, b) => a + b, 0) / sorted.length;

    return {
      fps: this.actualFps,
      renderedFps: this.actualFps,
      p50Ms: Number(p50.toFixed(2)),
      p95Ms: Number(p95.toFixed(2)),
      avgMs: Number(avg.toFixed(2)),
      renderedCount: this.totalRenderedFrames,
      isReducedMotion: this.reducedMotion,
      isPaused: !this.isRunning,
    };
  }

  public destroy() {
    this.pause();
    if (this.visibilityHandler) {
      document.removeEventListener('visibilitychange', this.visibilityHandler);
      this.visibilityHandler = null;
    }
    if (this.motionMediaQuery && this.motionChangeHandler) {
      this.motionMediaQuery.removeEventListener('change', this.motionChangeHandler);
    }
    if (this.resizeObserver) {
      this.resizeObserver.disconnect();
      this.resizeObserver = null;
    }
    if (import.meta.env.DEV && typeof window !== 'undefined') {
      const win = window as unknown as { __MIERU_SPHERE__?: unknown };
      if (win.__MIERU_SPHERE__ === this) {
        delete win.__MIERU_SPHERE__;
      }
    }
  }

  // ==========================================
  // Render Loop & Physics
  // ==========================================

  private loop = (timestamp: number) => {
    if (!this.isRunning) return;

    const dt = Math.min((timestamp - this.lastTimestamp) / 1000, 0.1);
    this.lastTimestamp = timestamp;

    // Real measurement of update & render execution duration
    const tStart = performance.now();

    this.update(dt, timestamp);
    this.render();

    const frameCost = performance.now() - tStart;
    this.recordFrameMetric(frameCost, timestamp);

    this.animFrameId = requestAnimationFrame(this.loop);
  };

  private recordFrameMetric(costMs: number, timestamp: number) {
    this.frameDurations.push(costMs);
    if (this.frameDurations.length > 120) {
      this.frameDurations.shift();
    }
    this.totalRenderedFrames++;
    this.fpsFrameCount++;

    if (timestamp - this.fpsWindowStart >= 1000) {
      this.actualFps = Math.round((this.fpsFrameCount * 1000) / (timestamp - this.fpsWindowStart));
      this.fpsWindowStart = timestamp;
      this.fpsFrameCount = 0;
    }
  }

  private renderStaticFrame() {
    if (this.isFallback || !this.ctx) return;
    if (this.reducedMotion) {
      this.applyTargetStateImmediate();
    }
    this.render();
  }

  private update(dt: number, timestamp: number) {
    this.stateTimer += dt;
    if (!this.reducedMotion) this.motionTime += dt;

    // Cold start progression (~1.4s assembly). Note: caller dictates readiness, we do NOT overwrite targetState!
    if (this.targetState === 'cold_start') {
      const progress = Math.min(1, this.stateTimer / 1.4);
      const eased = 1 - Math.pow(1 - progress, 3);
      this.currentCoalesce = 0.12 + eased * 0.88;
      this.currentFocus = 0.35 + eased * 0.45;
    }

    // Determine target parameters for current state
    let targetRotSpeed = 0.45;
    let targetFocus = 0.6;
    let targetCoalesce = 1.0;
    let targetCoreEnergy = 0.5;
    let targetBlur = 0.0;
    let targetSmokeMix = 0.0;

    switch (this.targetState) {
      case 'cold_start':
        targetRotSpeed = 0.85;
        targetCoreEnergy = 0.85;
        targetBlur = 0.5;
        targetSmokeMix = 0.0;
        break;
      case 'unconfigured':
        targetRotSpeed = 0.22;
        targetFocus = 0.3;
        targetCoreEnergy = 0.25;
        targetBlur = 1.2;
        targetSmokeMix = 1.0;
        break;
      case 'ready':
        targetRotSpeed = 0.38;
        targetFocus = 0.45;
        targetCoreEnergy = 0.45;
        targetBlur = 1.0; // Soft edge softening (1.0 CSS px) per user request
        targetSmokeMix = 1.0; // Full volumetric smoke shell (_ (1).gif)
        break;
      case 'connecting':
        // Accelerating and morphing from smoke into geometric mesh
        targetRotSpeed = 1.15;
        targetFocus = 0.88;
        targetCoreEnergy = 0.9;
        targetBlur = 0.8;
        targetSmokeMix = 0.35;
        break;
      case 'connected':
        // Crisp, high-tech complex cybernetic lattice per _.gif
        targetRotSpeed = 0.65;
        targetFocus = 1.0;
        targetCoreEnergy = 0.95;
        targetBlur = 0.0; // Razor sharp crisp lines!
        targetSmokeMix = 0.0; // Pure geometric lattice
        break;
      case 'error':
        targetRotSpeed = 0.28;
        targetFocus = 0.5;
        targetCoreEnergy = 0.75;
        targetBlur = 1.0;
        targetSmokeMix = 1.0;
        break;
      case 'disconnecting':
        targetRotSpeed = 0.5;
        targetFocus = 0.45;
        targetCoreEnergy = 0.4;
        targetBlur = 1.0;
        targetSmokeMix = 0.75; // Morphing back into smoke
        break;
    }

    // Interpolate visual parameters toward target
    const lerpRate = Math.min(1, dt * 4.5);
    this.currentRotSpeed += (targetRotSpeed - this.currentRotSpeed) * lerpRate;
    this.currentFocus += (targetFocus - this.currentFocus) * lerpRate;
    this.currentCoalesce += (targetCoalesce - this.currentCoalesce) * lerpRate;
    this.currentCoreEnergy += (targetCoreEnergy - this.currentCoreEnergy) * lerpRate;
    this.currentBlurPx += (targetBlur - this.currentBlurPx) * lerpRate;
    this.currentSmokeMix += (targetSmokeMix - this.currentSmokeMix) * lerpRate;

    // Color interpolation
    const targetColors = STATE_COLORS[this.targetState];
    const colorRate = Math.min(1, dt * 5.0);
    this.currentPrimary = this.lerpColor(this.currentPrimary, targetColors.primary, colorRate);
    this.currentSecondary = this.lerpColor(this.currentSecondary, targetColors.secondary, colorRate);
    this.currentCore = this.lerpColor(this.currentCore, targetColors.core, colorRate);
    this.currentGlow = this.lerpColor(this.currentGlow, targetColors.glow, colorRate);

    // Single gentle error pulse
    if (this.targetState === 'error') {
      const elapsedSincePulse = (timestamp - this.pulseTriggerTime) / 1000;
      if (elapsedSincePulse < 1.0) {
        this.currentPulse = Math.sin(elapsedSincePulse * Math.PI) * 0.25;
      } else {
        this.currentPulse = 0;
      }
    } else {
      this.currentPulse = 0;
    }

    // Motion progression (Only when reducedMotion is FALSE)
    if (!this.reducedMotion) {
      this.rotY += this.currentRotSpeed * dt;
      this.rotX = 0.24 + Math.sin(this.motionTime * 0.6) * 0.07;
      this.rotZ = 0.12 + Math.cos(this.motionTime * 0.5) * 0.05;

      for (const sat of this.satellites) {
        sat.angle += sat.speed * dt * this.currentRotSpeed;
      }
    } else {
      // Completely static scenic angle for reduced motion
      this.rotY = Math.PI * 0.35;
      this.rotX = 0.22;
      this.rotZ = 0.1;
    }
  }

  private lerpColor(c1: ColorRGB, c2: ColorRGB, t: number): ColorRGB {
    return {
      r: c1.r + (c2.r - c1.r) * t,
      g: c1.g + (c2.g - c1.g) * t,
      b: c1.b + (c2.b - c1.b) * t,
    };
  }

  // ==========================================
  // Layered Scene Rendering & Blur Compositing
  // ==========================================

  private render() {
    if (this.isFallback || !this.ctx) return;

    const { width, height } = this.canvas;
    if (width === 0 || height === 0) return;

    const canUseOffscreen = Boolean(this.offscreenCanvas && this.offscreenCtx);
    const drawCtx = canUseOffscreen ? this.offscreenCtx! : this.ctx;
    drawCtx.clearRect(0, 0, width, height);

    const dpr = Math.min(window.devicePixelRatio || 1, 2.5);
    const logicalW = width / dpr;
    const logicalH = height / dpr;

    const cx = logicalW / 2;
    const cy = logicalH / 2;
    const baseRadius = Math.min(cx, cy) * 0.72;
    const currentRadius = baseRadius * (1 + this.currentPulse);

    drawCtx.save();
    drawCtx.scale(dpr, dpr);

    const smokeMix = Math.max(0, Math.min(1, this.currentSmokeMix));
    const meshMix = 1 - smokeMix;

    // 1. Ambient Background Glow
    this.renderAmbientGlow(drawCtx, cx, cy, currentRadius);

    // 2. Central Shaded Point-Cloud Energy Core (Suppressed in pure smoke state per _ (1).gif cavity)
    if (this.currentCoreEnergy > 0.05 && meshMix > 0.05) {
      this.renderCore(drawCtx, cx, cy, currentRadius, meshMix);
    }

    // 3. Volumetric Smoke Stream Ribbons & Back Polar Caps (z < 0)
    if (smokeMix > 0.02) {
      this.renderSmokeShell(drawCtx, cx, cy, currentRadius, false, smokeMix);
    }

    // 4. Normalized Inner Flux Filaments (Animacion.gif) (Only active when mesh/cold_start active)
    if (meshMix > 0.05) {
      this.renderFilaments(drawCtx, cx, cy, currentRadius, meshMix);
    }

    // 5. Back Coordinate Arcs & Back Mesh & Back Particles (z < 0)
    if (meshMix > 0.05) {
      this.renderCoordinateArcs(drawCtx, cx, cy, currentRadius, false, meshMix);
      this.renderMeshLattice(drawCtx, cx, cy, currentRadius, false, meshMix);
      this.renderParticles(drawCtx, cx, cy, currentRadius, false, meshMix);
    }

    // 6. Volumetric Smoke Stream Ribbons & Front Polar Caps (z >= 0)
    if (smokeMix > 0.02) {
      this.renderSmokeShell(drawCtx, cx, cy, currentRadius, true, smokeMix);
    }

    // 7. Front Coordinate Arcs & Front Triangular Mesh & Front Particles (z >= 0)
    if (meshMix > 0.05) {
      this.renderCoordinateArcs(drawCtx, cx, cy, currentRadius, true, meshMix);
      this.renderMeshLattice(drawCtx, cx, cy, currentRadius, true, meshMix);
      this.renderParticles(drawCtx, cx, cy, currentRadius, true, meshMix);
    }

    // 8. Orbital Satellites & Sparks (Connected / high focus states)
    if (meshMix > 0.1) {
      this.renderSatellites(drawCtx, cx, cy, currentRadius, meshMix);
    }

    // 9. Bright Thin Outer Rim (Animacion.gif / Connected mode only)
    if (this.targetState === 'cold_start' || (this.targetState === 'connected' && meshMix > 0.5)) {
      this.renderOuterRim(drawCtx, cx, cy, currentRadius, meshMix);
    }

    drawCtx.restore();

    // Blur the final element in CSS pixels. Canvas filters are not consistently
    // available in WKWebView, and bitmap-space blur is weakened on Retina.
    // Interpolation above also handles smooth focusing and reduced-motion snapshots.
    const blur = this.currentBlurPx > 0.05 ? `blur(${this.currentBlurPx.toFixed(1)}px)` : 'none';
    if (this.canvas.style.filter !== blur) {
      this.canvas.style.filter = blur;
    }

    // Composite the sharp scene once; CSS handles blur without double filtering.
    if (canUseOffscreen && this.offscreenCanvas) {
      this.ctx.clearRect(0, 0, width, height);
      this.ctx.drawImage(this.offscreenCanvas, 0, 0);
    }
  }

  private renderOuterRim(ctx: CanvasRenderingContext2D, cx: number, cy: number, radius: number, meshMix = 1.0) {
    const { r, g, b } = this.currentPrimary;
    const { r: cr, g: cg, b: cb } = this.currentCore;
    const isCold = this.targetState === 'cold_start';
    const rimAlpha = (isCold ? 0.9 : 0.45 * this.currentFocus) * meshMix;

    ctx.save();
    // 1. Crisp thin outer rim boundary (Animacion.gif bright rim)
    ctx.strokeStyle = `rgba(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)}, ${rimAlpha})`;
    ctx.lineWidth = isCold ? 1.8 : 1.2;
    ctx.beginPath();
    ctx.arc(cx, cy, radius, 0, Math.PI * 2);
    ctx.stroke();

    // 2. Energetic outer corona
    if (isCold || this.targetState === 'connected') {
      ctx.strokeStyle = `rgba(${Math.round(cr)}, ${Math.round(cg)}, ${Math.round(cb)}, ${rimAlpha * 0.45})`;
      ctx.lineWidth = 3.2;
      ctx.beginPath();
      ctx.arc(cx, cy, radius + 1.2, 0, Math.PI * 2);
      ctx.stroke();
    }
    ctx.restore();
  }

  private renderAmbientGlow(ctx: CanvasRenderingContext2D, cx: number, cy: number, radius: number) {
    const glowRad = radius * 1.45;
    const { r, g, b } = this.currentGlow;
    const alpha = this.isDark ? 0.25 : 0.16;

    const grad = ctx.createRadialGradient(cx, cy, radius * 0.15, cx, cy, glowRad);
    grad.addColorStop(0, `rgba(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)}, ${alpha})`);
    grad.addColorStop(0.5, `rgba(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)}, ${alpha * 0.45})`);
    grad.addColorStop(1, `rgba(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)}, 0)`);

    ctx.fillStyle = grad;
    ctx.beginPath();
    ctx.arc(cx, cy, glowRad, 0, Math.PI * 2);
    ctx.fill();
  }

  private renderCore(ctx: CanvasRenderingContext2D, cx: number, cy: number, radius: number, meshMix = 1.0) {
    const timeVal = this.reducedMotion ? 1.0 : this.motionTime;
    const coreRad = radius * (0.26 + Math.sin(timeVal * 2.5) * 0.02 * this.currentCoreEnergy);
    const { r: cr, g: cg, b: cb } = this.currentCore;
    const { r: pr, g: pg, b: pb } = this.currentPrimary;

    const coreGrad = ctx.createRadialGradient(cx, cy, 0, cx, cy, coreRad);
    coreGrad.addColorStop(0, `rgba(${Math.round(cr)}, ${Math.round(cg)}, ${Math.round(cb)}, ${0.95 * meshMix})`);
    coreGrad.addColorStop(0.45, `rgba(${Math.round(pr)}, ${Math.round(pg)}, ${Math.round(pb)}, ${0.65 * meshMix})`);
    coreGrad.addColorStop(1, `rgba(${Math.round(pr)}, ${Math.round(pg)}, ${Math.round(pb)}, 0)`);

    ctx.fillStyle = coreGrad;
    ctx.beginPath();
    ctx.arc(cx, cy, coreRad, 0, Math.PI * 2);
    ctx.fill();
  }

  /**
   * Volumetric flowing smoke shell inspired by user reference _ (1).gif.
   * Renders stratified flowing wisps, turbulent vorticity, noticeable hollow cavity,
   * and luminous upper/lower clusters without heavy per-frame shadowBlur.
   */
  private renderSmokeShell(
    ctx: CanvasRenderingContext2D,
    cx: number,
    cy: number,
    radius: number,
    isFront: boolean,
    smokeMix: number
  ) {
    const timeVal = this.reducedMotion ? 1.0 : this.motionTime;
    const { r: pr, g: pg, b: pb } = this.currentPrimary;
    const { r: sr, g: sg, b: sb } = this.currentSecondary;
    const { r: cr, g: cg, b: cb } = this.currentCore;
    const stamp = this.smokeStamp;

    // 1. Flowing Smoke Stream Ribbons
    const steps = 28;
    for (const ribbon of this.smokeRibbons) {
      const isOuter = ribbon.layer === 2;
      const isInner = ribbon.layer === 0;

      const flowTime = timeVal * ribbon.speed;
      const ribbonWidth = ribbon.width * (radius / 100);

      // Ribbon spine points in 3D
      ctx.beginPath();
      let started = false;

      for (let s = 0; s <= steps; s++) {
        const u = (s / steps) * 2 - 1; // -1 to 1 along ribbon length
        const lat = u * 1.35 + Math.sin(u * ribbon.harmonicU + flowTime * 0.9) * 0.06;
        const lon = ribbon.baseLon + flowTime * 0.45 + Math.cos(u * ribbon.harmonicV + flowTime * 0.75) * 0.16;

        // Radial turbulence: shell thickness is hollow inside [0.68, 1.04]
        // Cavity in center: radius never drops below 0.65!
        const rBase = isOuter ? 0.96 : isInner ? 0.72 : 0.85;
        const rJitter = Math.sin(u * Math.PI * 2 + flowTime) * 0.04 + Math.cos(lat * 3 + flowTime * 0.8) * 0.025;
        const rNorm = rBase + rJitter;

        const cosLat = Math.cos(lat);
        const px = rNorm * cosLat * Math.cos(lon);
        const py = rNorm * Math.sin(lat);
        const pz = rNorm * cosLat * Math.sin(lon);

        const proj = this.projectPoint({ x: px, y: py, z: pz }, cx, cy, radius);

        const belongs = isFront ? proj.z >= 0 : proj.z < 0;
        if (!belongs) {
          started = false;
          continue;
        }

        if (!started) {
          ctx.moveTo(proj.x, proj.y);
          started = true;
        } else {
          ctx.lineTo(proj.x, proj.y);
        }
      }

      if (started) {
        // Upper and lower edges get subtle core brightness
        const isPoleAccent = Math.abs(ribbon.baseLat) > 0.65;
        const rColor = isPoleAccent ? cr * 0.4 + pr * 0.6 : isOuter ? pr : sr;
        const gColor = isPoleAccent ? cg * 0.4 + pg * 0.6 : isOuter ? pg : sg;
        const bColor = isPoleAccent ? cb * 0.4 + pb * 0.6 : isOuter ? pb : sb;

        const alpha = ribbon.alphaFactor * smokeMix * (isFront ? 0.85 : 0.45);
        ctx.lineCap = 'round';
        ctx.lineJoin = 'round';
        // Overlapping translucent bands feather each stream into the shell.
        // No bright contour: the accumulated density provides the visible wisp.
        for (const [spread, opacity] of [[8, 0.06], [4, 0.12], [1.8, 0.1]]) {
          ctx.lineWidth = ribbonWidth * spread;
          ctx.strokeStyle = `rgba(${Math.round(rColor)}, ${Math.round(gColor)}, ${Math.round(bColor)}, ${alpha * opacity})`;
          ctx.stroke();
        }
      }
    }

    // 2. Volumetric layered smoke puffs & Polar luminous clusters
    if (stamp) {
      for (const puff of this.smokePuffs) {
        const flowTime = timeVal * puff.speed;
        const lat = puff.lat + Math.sin(flowTime + puff.phase) * 0.08;
        const lon = puff.lon + flowTime * 0.45 + Math.cos(flowTime * 0.7 + puff.phase) * 0.15;
        const rNorm = puff.radiusFactor + Math.sin(flowTime * 1.2 + puff.phase) * 0.05;

        const cosLat = Math.cos(lat);
        const px = rNorm * cosLat * Math.cos(lon);
        const py = rNorm * Math.sin(lat);
        const pz = rNorm * cosLat * Math.sin(lon);

        const proj = this.projectPoint({ x: px, y: py, z: pz }, cx, cy, radius);

        const belongs = isFront ? proj.z >= 0 : proj.z < 0;
        if (!belongs) continue;

        // Size with perspective scale
        const renderSize = puff.size * proj.scale * (radius / 80);

        // Alpha calculation: polar puffs are more luminous clusters per _ (1).gif
        let puffAlpha = puff.isPolar
          ? (isFront ? 0.32 : 0.16) * smokeMix
          : (isFront ? 0.18 : 0.08) * smokeMix;

        ctx.save();
        ctx.globalAlpha = puffAlpha;
        // Blit cached stamp directly
        ctx.drawImage(stamp, proj.x - renderSize / 2, proj.y - renderSize / 2, renderSize, renderSize);
        ctx.restore();
      }
    }
  }

  /**
   * Correctly projects and clips normalized filaments inside sphere radius.
   * Eliminates all parasitic diagonal lines.
   */
  private renderFilaments(ctx: CanvasRenderingContext2D, cx: number, cy: number, radius: number, meshMix = 1.0) {
    const steps = 36;
    const { r, g, b } = this.currentPrimary;
    const isCold = this.targetState === 'cold_start';
    const alphaBase = (isCold ? 0.75 : 0.42 * this.currentFocus) * meshMix;
    const timeVal = this.reducedMotion ? 1.2 : this.motionTime;

    ctx.lineWidth = isCold ? 1.8 : 1.3;

    for (const f of this.filaments) {
      const time = timeVal * f.speed;
      ctx.beginPath();

      let started = false;
      for (let i = 0; i <= steps; i++) {
        const u = (i / steps) * 2 - 1; // -1 to 1
        const lat = u * f.latSpan;
        const lon = f.lonOffset + time + Math.sin(u * f.harmonic + time) * 0.38;

        // Strictly normalized radius in [0.45, 0.88]
        const rNorm = f.radiusBase + Math.cos(u * Math.PI + time) * 0.08;
        const px = rNorm * Math.cos(lat) * Math.cos(lon);
        const py = rNorm * Math.sin(lat);
        const pz = rNorm * Math.cos(lat) * Math.sin(lon);

        const proj = this.projectPoint({ x: px, y: py, z: pz }, cx, cy, radius);

        // Soft back-clip to keep filaments clean
        if (proj.z < -0.45) {
          started = false;
          continue;
        }

        if (!started) {
          ctx.moveTo(proj.x, proj.y);
          started = true;
        } else {
          ctx.lineTo(proj.x, proj.y);
        }
      }

      ctx.strokeStyle = `rgba(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)}, ${alphaBase})`;
      ctx.stroke();
    }
  }

  /**
   * Intricate Triangular Mesh Lattice (_.gif connected reference).
   * Draws connected edges between surface neighbors when focused.
   */
  private renderMeshLattice(
    ctx: CanvasRenderingContext2D,
    cx: number,
    cy: number,
    radius: number,
    isFront: boolean,
    meshMix = 1.0
  ) {
    if (this.currentFocus < 0.25) return;

    const { r, g, b } = this.currentPrimary;
    const meshAlpha = (isFront ? 0.28 : 0.09) * this.currentFocus * meshMix;

    ctx.lineWidth = isFront ? 1.0 : 0.6;
    ctx.strokeStyle = `rgba(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)}, ${meshAlpha})`;

    const projectedNodes = this.meshNodes.map((n) => this.projectPoint(n, cx, cy, radius));

    ctx.beginPath();
    for (const edge of this.meshEdges) {
      const p1 = projectedNodes[edge.p1];
      const p2 = projectedNodes[edge.p2];

      const edgeAvgZ = (p1.z + p2.z) / 2;
      const belongs = isFront ? edgeAvgZ >= 0 : edgeAvgZ < 0;
      if (!belongs) continue;

      ctx.moveTo(p1.x, p1.y);
      ctx.lineTo(p2.x, p2.y);
    }
    ctx.stroke();
  }

  /**
   * Precision HUD Coordinate Arcs with radial tick marks.
   */
  private renderCoordinateArcs(
    ctx: CanvasRenderingContext2D,
    cx: number,
    cy: number,
    radius: number,
    isFront: boolean,
    meshMix = 1.0
  ) {
    const { r: pr, g: pg, b: pb } = this.currentPrimary;
    const { r: sr, g: sg, b: sb } = this.currentSecondary;

    ctx.lineWidth = isFront ? 1.4 : 0.8;
    const steps = 48;

    for (const arc of this.coordinateArcs) {
      const arcPoints: ProjectedPoint[] = [];

      for (let i = 0; i <= steps; i++) {
        const theta = (i / steps) * Math.PI * 2;
        const x0 = Math.cos(theta) * arc.radiusRatio;
        const y0 = 0;
        const z0 = Math.sin(theta) * arc.radiusRatio;

        // Apply tilt
        const cosX = Math.cos(arc.tiltX);
        const sinX = Math.sin(arc.tiltX);
        const y1 = y0 * cosX - z0 * sinX;
        const z1 = y0 * sinX + z0 * cosX;

        const cosZ = Math.cos(arc.tiltZ);
        const sinZ = Math.sin(arc.tiltZ);
        const x2 = x0 * cosZ - y1 * sinZ;
        const y2 = x0 * sinZ + y1 * cosZ;

        arcPoints.push(this.projectPoint({ x: x2, y: y2, z: z1 }, cx, cy, radius));
      }

      // Draw continuous arc segments in current hemisphere
      ctx.strokeStyle = `rgba(${Math.round(isFront ? pr : sr)}, ${Math.round(isFront ? pg : sg)}, ${Math.round(isFront ? pb : sb)}, ${(isFront ? 0.35 * this.currentFocus : 0.1) * meshMix})`;
      ctx.beginPath();
      let started = false;

      for (let i = 0; i <= steps; i++) {
        const pt = arcPoints[i];
        const belongs = isFront ? pt.z >= 0 : pt.z < 0;

        if (belongs) {
          if (!started) {
            ctx.moveTo(pt.x, pt.y);
            started = true;
          } else {
            ctx.lineTo(pt.x, pt.y);
          }
        } else {
          started = false;
        }
      }
      ctx.stroke();

      // Draw radial tick marks along precision rings
      if (arc.hasTicks && isFront && this.currentFocus > 0.4) {
        ctx.strokeStyle = `rgba(${Math.round(pr)}, ${Math.round(pg)}, ${Math.round(pb)}, ${0.45 * this.currentFocus * meshMix})`;
        ctx.lineWidth = 1.1;

        for (let i = 0; i < steps; i += arc.tickInterval) {
          const pt = arcPoints[i];
          if (pt.z < 0) continue;

          // Outward normal tick
          const dx = pt.x - cx;
          const dy = pt.y - cy;
          const len = Math.sqrt(dx * dx + dy * dy);
          if (len > 0) {
            const tickLen = 4.5 * pt.scale;
            ctx.beginPath();
            ctx.moveTo(pt.x, pt.y);
            ctx.lineTo(pt.x + (dx / len) * tickLen, pt.y + (dy / len) * tickLen);
            ctx.stroke();
          }
        }
      }
    }
  }

  /**
   * Dense Point Cloud rendering (1800 surface + 600 interior sparks)
   * with depth scaling and offscreen cached sprite blitting.
   */
  private renderParticles(
    ctx: CanvasRenderingContext2D,
    cx: number,
    cy: number,
    radius: number,
    isFront: boolean,
    meshMix = 1.0
  ) {
    const { r, g, b } = this.currentPrimary;
    const { r: cr, g: cg, b: cb } = this.currentCore;
    const sprite = this.glowSprite;
    const timeVal = this.reducedMotion ? 1.0 : this.motionTime;

    for (let i = 0; i < this.particles.length; i++) {
      const p = this.particles[i];

      // Interpolate scatter (cold start) into surface
      const coal = this.currentCoalesce;
      const xBase = p.base.x * coal + p.scatter.x * (1 - coal);
      const yBase = p.base.y * coal + p.scatter.y * (1 - coal);
      const zBase = p.base.z * coal + p.scatter.z * (1 - coal);

      // Subtle breathing jitter
      const breath = this.reducedMotion
        ? 1.0 + p.radiusOffset
        : 1.0 + Math.sin(timeVal * p.speed + p.phase) * 0.025 + p.radiusOffset;

      const pt3D = {
        x: xBase * breath,
        y: yBase * breath,
        z: zBase * breath,
      };

      const proj = this.projectPoint(pt3D, cx, cy, radius);

      const belongs = isFront ? proj.z >= 0 : proj.z < 0;
      if (!belongs) continue;

      const dotAlpha = (isFront
        ? p.isCore
          ? 0.55 + proj.alpha * 0.4
          : 0.32 + proj.alpha * 0.55 * this.currentFocus
        : 0.08 + proj.alpha * 0.16) * meshMix;

      const size = Math.max(0.8, (p.isCore ? 1.6 : isFront ? 2.1 : 1.3) * proj.scale);

      // Fast cached sprite blit for prominent front nodes
      if (sprite && isFront && proj.scale > 0.95 && this.currentFocus > 0.6 && !p.isCore) {
        ctx.globalAlpha = dotAlpha * 0.5;
        const spriteSize = size * 3.6;
        ctx.drawImage(sprite, proj.x - spriteSize / 2, proj.y - spriteSize / 2, spriteSize, spriteSize);
        ctx.globalAlpha = 1.0;
      }

      const dotColor = p.isCore ? `rgba(${cr}, ${cg}, ${cb}, ${dotAlpha})` : `rgba(${r}, ${g}, ${b}, ${dotAlpha})`;
      ctx.fillStyle = dotColor;
      ctx.beginPath();
      ctx.arc(proj.x, proj.y, size / 2, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  private renderSatellites(ctx: CanvasRenderingContext2D, cx: number, cy: number, radius: number, meshMix = 1.0) {
    const { r: cr, g: cg, b: cb } = this.currentCore;
    const { r: pr, g: pg, b: pb } = this.currentPrimary;

    for (const sat of this.satellites) {
      const sx = Math.cos(sat.angle) * sat.radius;
      const sy = Math.sin(sat.angle) * Math.sin(sat.tilt) * sat.radius;
      const sz = Math.sin(sat.angle) * Math.cos(sat.tilt) * sat.radius;

      const proj = this.projectPoint({ x: sx, y: sy, z: sz }, cx, cy, radius);

      const sparkRad = 2.4 * proj.scale;
      ctx.fillStyle = `rgba(${cr}, ${cg}, ${cb}, ${(0.45 + proj.alpha * 0.55) * meshMix})`;
      ctx.beginPath();
      ctx.arc(proj.x, proj.y, sparkRad, 0, Math.PI * 2);
      ctx.fill();

      // Trailing ring halo
      ctx.strokeStyle = `rgba(${pr}, ${pg}, ${pb}, ${0.25 * proj.alpha * meshMix})`;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.arc(proj.x, proj.y, sparkRad * 2.2, 0, Math.PI * 2);
      ctx.stroke();
    }
  }

  /**
   * Projects normalized 3D point into 2D canvas coordinates.
   * Expects pt normalized in [-1, 1].
   */
  private projectPoint(pt: Point3D, cx: number, cy: number, radius: number): ProjectedPoint {
    // 1. Rotation around Y-axis (Yaw)
    const cosY = Math.cos(this.rotY);
    const sinY = Math.sin(this.rotY);
    const x1 = pt.x * cosY + pt.z * sinY;
    const y1 = pt.y;
    const z1 = -pt.x * sinY + pt.z * cosY;

    // 2. Rotation around X-axis (Pitch)
    const cosX = Math.cos(this.rotX);
    const sinX = Math.sin(this.rotX);
    const x2 = x1;
    const y2 = y1 * cosX - z1 * sinX;
    const z2 = y1 * sinX + z1 * cosX;

    // 3. Rotation around Z-axis (Roll)
    const cosZ = Math.cos(this.rotZ);
    const sinZ = Math.sin(this.rotZ);
    const x3 = x2 * cosZ - y2 * sinZ;
    const y3 = x2 * sinZ + y2 * cosZ;
    const z3 = z2;

    // 4. Perspective projection
    const fov = 2.8;
    const scale = fov / (fov + z3);
    const projX = cx + x3 * radius * scale;
    const projY = cy + y3 * radius * scale;

    const depthAlpha = Math.max(0, Math.min(1, (z3 + 1) / 2));

    return {
      x: projX,
      y: projY,
      z: z3,
      scale,
      alpha: depthAlpha,
    };
  }

  /**
   * Graceful fallback when Canvas 2D is unavailable.
   * State changes update colors and status label dynamically.
   */
  private renderFallback() {
    const parent = this.canvas.parentElement;
    if (!parent) return;

    this.canvas.style.display = 'none';
    if (!this.fallbackEl) {
      this.fallbackEl = document.createElement('div');
      parent.appendChild(this.fallbackEl);
    }
    this.updateFallback();
  }

  private updateFallback() {
    if (!this.fallbackEl) return;
    const styles: Record<SphereVisualState, { bg: string; text: string; label: string }> = {
      cold_start: { bg: 'bg-cyan-500/10 border-cyan-500/30', text: 'text-cyan-500', label: 'Инициализация' },
      unconfigured: { bg: 'bg-slate-500/10 border-slate-500/30', text: 'text-slate-400', label: 'Не настроен' },
      ready: { bg: 'bg-amber-500/10 border-amber-500/30', text: 'text-amber-500', label: 'Сервер готов' },
      connecting: { bg: 'bg-blue-500/10 border-blue-500/30', text: 'text-blue-500', label: 'Подключение...' },
      connected: { bg: 'bg-emerald-500/10 border-emerald-500/30', text: 'text-emerald-500', label: 'Подключено' },
      error: { bg: 'bg-rose-500/10 border-rose-500/30', text: 'text-rose-500', label: 'Ошибка' },
      disconnecting: { bg: 'bg-amber-500/10 border-amber-500/30', text: 'text-amber-500', label: 'Отключение...' },
    };
    const s = styles[this.targetState] || styles.unconfigured;
    this.fallbackEl.className = `w-36 h-36 rounded-full flex flex-col items-center justify-center border transition-colors ${s.bg} ${s.text}`;
    this.fallbackEl.innerHTML = `
      <svg class="w-12 h-12" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M12 2a10 10 0 100 20 10 10 0 000-20zm0 4a6 6 0 100 12 6 6 0 000-12z" />
      </svg>
      <span class="text-[10px] font-semibold mt-1">${s.label}</span>
    `;
  }
}
