// AX8850 AI BOX ID mockup for 3D printing (Bambu)
// Units: mm

$fn = 64;

// Overall size (from hardware plan)
box_len = 155;
box_wid = 78;
box_hei = 18;
corner_r = 10;

// Shell settings
wall = 2.2;
base_h = 12.0;
lid_h = box_hei - base_h;
fit_gap = 0.25; // print tolerance between lid and base

// Port geometry (from plan)
rj45_w = 16.2;
rj45_h = 14.8;
reset_d = 1.4;
led_d = 2.0;
tf_slot_w = 14.0;
tf_slot_h = 2.6;

// Port placement offsets (from center)
rj45_x = -52;
rj45_z = 2.0;

tf_x = 48;
tf_z = -1.0;

reset_x = 58;
reset_z = 3.0;

led_x = 64;
led_z = 5.0;

// Inner posts for screw placeholders
post_d = 5.0;
post_h = 6.0;
post_hole_d = 2.6;
post_offset_x = 60;
post_offset_y = 28;

module rounded_box(l, w, h, r) {
    hull() {
        for (x = [-l/2 + r, l/2 - r])
            for (y = [-w/2 + r, w/2 - r])
                translate([x, y, 0]) cylinder(h = h, r = r);
    }
}

module base_shell() {
    difference() {
        rounded_box(box_len, box_wid, base_h, corner_r);

        // Hollow cavity
        translate([0, 0, wall])
            rounded_box(box_len - 2 * wall, box_wid - 2 * wall, base_h, corner_r - wall * 0.6);

        // RJ45 cutout on left side
        translate([-box_len / 2 - 0.5, 0, wall + rj45_z])
            rotate([0, 90, 0])
                cube([rj45_w, rj45_h, 3], center = true);

        // TF slot on right side
        translate([box_len / 2 - 0.6, 0, wall + tf_z])
            rotate([0, 90, 0])
                cube([tf_slot_w, tf_slot_h, 3], center = true);

        // Reset pinhole on right side
        translate([box_len / 2 - 0.6, 14, wall + reset_z])
            rotate([0, 90, 0])
                cylinder(h = 3, d = reset_d, center = true);

        // LED hole on right side
        translate([box_len / 2 - 0.6, -14, wall + led_z])
            rotate([0, 90, 0])
                cylinder(h = 3, d = led_d, center = true);
    }

    // Screw post placeholders
    for (x = [-post_offset_x, post_offset_x])
        for (y = [-post_offset_y, post_offset_y])
            translate([x, y, wall])
                difference() {
                    cylinder(h = post_h, d = post_d);
                    cylinder(h = post_h + 0.2, d = post_hole_d);
                }
}

module lid_shell() {
    difference() {
        rounded_box(box_len, box_wid, lid_h, corner_r);

        // Hollow lid
        translate([0, 0, wall])
            rounded_box(box_len - 2 * wall, box_wid - 2 * wall, lid_h, corner_r - wall * 0.6);
    }

    // Add lip for fitting into base
    translate([0, 0, 0])
        difference() {
            rounded_box(box_len - 2 * wall - fit_gap * 2, box_wid - 2 * wall - fit_gap * 2, wall + 1.0, corner_r - wall);
            translate([0, 0, 0])
                rounded_box(box_len - 4 * wall - fit_gap * 2, box_wid - 4 * wall - fit_gap * 2, wall + 1.2, corner_r - 2 * wall);
        }
}

// Export helpers
module assembly_view() {
    color([0.8, 0.8, 0.82, 1.0]) base_shell();
    translate([0, 0, base_h + 0.6])
        color([0.72, 0.72, 0.75, 0.9]) lid_shell();
}

module export_base_only() { base_shell(); }
module export_lid_only() { lid_shell(); }

// Default preview
assembly_view();
