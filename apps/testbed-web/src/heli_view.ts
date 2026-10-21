// Police helicopters (Air-1 …) in the 3D scene: a helicopter model (fuselage, tail boom,
// spinning rotors, red/blue strobes) instead of the scaled-up patrol box, and a spotlight cone
// with a ground pool on the racer it tracks (wp_wasm police_helicopters()). All in sim space
// (+Z up), parented under the scene's world group.
import * as THREE from "three";

/** Floats per helicopter in police_helicopters(): [x, y, z, aim_x, aim_y, aim_z, flags]. */
export const HELI_STRIDE = 7;
/** police_helicopters() flag: tracking the player. */
export const HELI_TRACKS_PLAYER = 1;

const BODY = 0x1a2233;
const TRIM = 0xe9ebf0;
const BEAM = 0xfff2b0;

interface Heli {
  obj: THREE.Group;
  rotor: THREE.Object3D;
  tail: THREE.Object3D;
  red: THREE.MeshStandardMaterial;
  blue: THREE.MeshStandardMaterial;
}

interface Beam {
  cone: THREE.Mesh;
  pool: THREE.Mesh;
}

export class HeliView {
  private readonly helis: Heli[] = [];
  private readonly beams: Beam[] = [];

  constructor(private readonly parent: THREE.Object3D) {}

  private makeHeli(): Heli {
    const g = new THREE.Group();
    const body = new THREE.MeshStandardMaterial({ color: BODY });
    const trim = new THREE.MeshStandardMaterial({ color: TRIM });
    const cabin = new THREE.Mesh(new THREE.SphereGeometry(1, 16, 12), body);
    cabin.scale.set(1.3, 2.4, 1.2);
    const stripe = new THREE.Mesh(new THREE.BoxGeometry(2.62, 2.2, 0.3), trim);
    stripe.position.z = 0.1;
    const boom = new THREE.Mesh(new THREE.CylinderGeometry(0.22, 0.35, 5.5, 8), body);
    boom.position.set(0, -4.6, 0.3); // cylinder axis is +Y: along the body
    const fin = new THREE.Mesh(new THREE.BoxGeometry(0.15, 1.0, 1.4), body);
    fin.position.set(0, -7.2, 0.9);
    const skids = new THREE.Mesh(new THREE.BoxGeometry(2.4, 3.6, 0.1), trim);
    skids.position.z = -1.4;
    const mast = new THREE.Mesh(new THREE.CylinderGeometry(0.12, 0.12, 0.6, 6), body);
    mast.rotation.x = Math.PI / 2;
    mast.position.z = 1.4;
    const rotor = new THREE.Group();
    rotor.position.z = 1.7;
    const bladeMat = new THREE.MeshStandardMaterial({ color: 0x0c0f14 });
    for (let i = 0; i < 4; i++) {
      const blade = new THREE.Mesh(new THREE.BoxGeometry(0.35, 11, 0.06), bladeMat);
      blade.rotation.z = (i * Math.PI) / 4;
      rotor.add(blade);
    }
    // Faint disc so the rotor reads at a distance even when the blades alias.
    const disc = new THREE.Mesh(
      new THREE.CircleGeometry(5.5, 32),
      new THREE.MeshBasicMaterial({ color: 0x9aa4b4, transparent: true, opacity: 0.12, side: THREE.DoubleSide, depthWrite: false }),
    );
    rotor.add(disc);
    const tail = new THREE.Group();
    tail.position.set(0.2, -7.2, 0.9);
    const tailBlade = new THREE.Mesh(new THREE.BoxGeometry(0.05, 1.8, 0.2), bladeMat);
    tail.add(tailBlade);
    const red = new THREE.MeshStandardMaterial({ color: 0x661018 });
    const blue = new THREE.MeshStandardMaterial({ color: 0x102266 });
    const lightR = new THREE.Mesh(new THREE.SphereGeometry(0.22, 8, 6), red);
    lightR.position.set(-0.9, -1.2, -1.1);
    const lightB = new THREE.Mesh(new THREE.SphereGeometry(0.22, 8, 6), blue);
    lightB.position.set(0.9, -1.2, -1.1);
    g.add(cabin, stripe, boom, fin, skids, mast, rotor, tail, lightR, lightB);
    this.parent.add(g);
    return { obj: g, rotor, tail, red, blue };
  }

  private makeBeam(): Beam {
    // Unit-height cone with its tip at the origin, opening down -Z; scaled per frame.
    const coneGeo = new THREE.ConeGeometry(1, 1, 24, 1, true);
    coneGeo.rotateX(Math.PI / 2);
    coneGeo.translate(0, 0, -0.5);
    const cone = new THREE.Mesh(
      coneGeo,
      new THREE.MeshBasicMaterial({ color: BEAM, transparent: true, opacity: 0.1, side: THREE.DoubleSide, depthWrite: false, blending: THREE.AdditiveBlending }),
    );
    const pool = new THREE.Mesh(
      new THREE.CircleGeometry(1, 32),
      new THREE.MeshBasicMaterial({ color: BEAM, transparent: true, opacity: 0.35, depthWrite: false, blending: THREE.AdditiveBlending }),
    );
    pool.renderOrder = 2;
    this.parent.add(cone, pool);
    return { cone, pool };
  }

  /**
   * Draw this frame. `units` = police_units() rows for helicopters only ([x, y, z, yaw]),
   * `beams` = police_helicopters() (spotlights of the ones on station), `timeS` drives the
   * rotor spin and strobes.
   */
  update(units: number[][], beams: Float32Array, timeS: number): void {
    while (this.helis.length < units.length) this.helis.push(this.makeHeli());
    this.helis.forEach((h, i) => {
      h.obj.visible = i < units.length;
      if (i >= units.length) return;
      const [x, y, z, yaw] = units[i];
      h.obj.position.set(x, y, z);
      h.obj.rotation.z = yaw;
      h.rotor.rotation.z = timeS * 28;
      h.tail.rotation.x = timeS * 60;
      const phase = (timeS * 1.5 + i * 0.3) % 1;
      h.red.emissive.setHex(phase < 0.12 ? 0xff2040 : 0x000000);
      h.blue.emissive.setHex(phase >= 0.5 && phase < 0.62 ? 0x2060ff : 0x000000);
    });

    const n = beams.length / HELI_STRIDE;
    while (this.beams.length < n) this.beams.push(this.makeBeam());
    const from = new THREE.Vector3();
    const to = new THREE.Vector3();
    const down = new THREE.Vector3(0, 0, -1);
    this.beams.forEach((b, i) => {
      const on = i < n;
      b.cone.visible = on;
      b.pool.visible = on;
      if (!on) return;
      const o = i * HELI_STRIDE;
      from.set(beams[o], beams[o + 1], beams[o + 2] - 1.2);
      to.set(beams[o + 3], beams[o + 4], beams[o + 5]);
      const len = from.distanceTo(to);
      // Pool radius: a tight spot on the car, a little wobble so it reads as hand-held.
      const r = 7 + Math.sin(timeS * 1.7 + i) * 0.6;
      b.cone.position.copy(from);
      b.cone.quaternion.setFromUnitVectors(down, to.clone().sub(from).normalize());
      b.cone.scale.set(r, r, len);
      b.pool.position.set(to.x, to.y, to.z + 0.15);
      b.pool.scale.setScalar(r);
      // Brighter when the light is on the player.
      (b.pool.material as THREE.MeshBasicMaterial).opacity = beams[o + 6] & HELI_TRACKS_PLAYER ? 0.45 : 0.3;
    });
  }
}
