pub const DEFAULT_KEY_AREA_LAYOUT_TOML: &str = r#"
name = "default"

[[elements]]
height = 6
spacing = 1
# 13 * 8 + 16 + 13 = 133
elements = ["k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8", "k9", "k10", "k11", "k12", "k13", "k14:16"]

[[elements]]
height = 6
spacing = 1
# 12 * 8 + 2 * 12 + 13 = 133
elements = ["k15:12", "k16", "k17", "k18", "k19", "k20", "k21", "k22", "k23", "k24", "k25", "k26", "k27", "k28:12"]

[[elements]]
height = 6
spacing = 1
# 13 + 11 * 8 + 20 + 12 = 133
elements = ["k29:13", "k30", "k31", "k32", "k33", "k34", "k35", "k36", "k37", "k38", "k39", "k40", "k41:20"]

[[elements]]
height = 6
spacing = 1
# 18 + 10 * 8 + 24 + 11 = 133
elements = ["k42:18", "k43", "k44", "k45", "k46", "k47", "k48", "k49", "k50", "k51", "k52", "k53:24"]

[[elements]]
height = 6
spacing = 1
# 8 + 10 + 8 + 10 + 39 + 10 + 8 + 8 + 24 + 8 = 133
elements = ["p:8", "k54:10", "k55", "k56:10", "k57:39", "k58:10", "k59", "k60", "p:24"]

[key_mappings]
k1 = "k_grave_accent"
k2 = "k_one"
k3 = "k_two"
k4 = "k_three"
k5 = "k_four"
k6 = "k_five"
k7 = "k_six"
k8 = "k_seven"
k9 = "k_eight"
k10 = "k_nine"
k11 = "k_zero"
k12 = "k_hyphen"
k13 = "k_equals"
k14 = "k_backspace"
k15 = "k_tab"
k16 = "k_q"
k17 = "k_w"
k18 = "k_e"
k19 = "k_r"
k20 = "k_t"
k21 = "k_y"
k22 = "k_u"
k23 = "k_i"
k24 = "k_o"
k25 = "k_p"
k26 = "k_open_bracket"
k27 = "k_close_bracket"
k28 = "k_backslash"
k29 = "k_escape"
k30 = "k_a"
k31 = "k_s"
k32 = "k_d"
k33 = "k_f"
k34 = "k_g"
k35 = "k_h"
k36 = "k_j"
k37 = "k_k"
k38 = "k_l"
k39 = "k_semicolon"
k40 = "k_apostrophe"
k41 = "k_enter"
k42 = "k_left_shift"
k43 = "k_z"
k44 = "k_x"
k45 = "k_c"
k46 = "k_v"
k47 = "k_b"
k48 = "k_n"
k49 = "k_m"
k50 = "k_comma"
k51 = "k_dot"
k52 = "k_slash"
k53 = "k_right_shift"
k54 = "k_left_ctrl"
k55 = "k_left_super"
k56 = "k_left_alt"
k57 = "k_space"
k58 = "k_right_alt"
k59 = "k_print"
k60 = "k_right_ctrl"
"#;

pub const DEFAULT_KEY_SET_TOML: &str = r#"
name = "default"

[keys.k_grave_accent]
p = {c = "`"}
[[keys.k_grave_accent.s]]
c = "~"

[keys.k_one]
p = {c = "1"}
[[keys.k_one.s]]
c = "!"

[keys.k_two]
p = {c = "2"}
[[keys.k_two.s]]
c = "@"

[keys.k_three]
p = {c = "3"}

[keys.k_four]
p = {c = "4"}

[keys.k_five]
p = {c = "5"}

[keys.k_six]
p = {c = "6"}

[keys.k_seven]
p = {c = "7"}

[keys.k_eight]
p = {c = "8"}

[keys.k_nine]
p = {c = "9"}

[keys.k_zero]
p = {c = "0"}

[keys.k_hyphen]
p = {c = "-"}

[keys.k_equals]
p = {c = "="}

[keys.k_backspace]
p = {ks = 0xff08}

[keys.k_tab]
p = {ks = 0xff09}

[keys.k_q]
p = {c = "q"}

[keys.k_w]
p = {c = "w"}

[keys.k_e]
p = {c = "e"}

[keys.k_r]
p = {c = "r"}

[keys.k_t]
p = {c = "t"}

[keys.k_y]
p = {c = "y"}

[keys.k_u]
p = {c = "u"}

[keys.k_i]
p = {c = "i"}

[keys.k_o]
p = {c = "o"}

[keys.k_p]
p = {c = "p"}

[keys.k_open_bracket]
p = {c = "["}

[keys.k_close_bracket]
p = {c = "]"}

[keys.k_backslash]
p = {c = "\\"}

[keys.k_caps_lock]
p = {ks = 0xffe5}

[keys.k_escape]
p = {ks = 0xff1b}

[keys.k_a]
p = {c = "a"}

[keys.k_s]
p = {c = "s"}

[keys.k_d]
p = {c = "d"}

[keys.k_f]
p = {c = "f"}

[keys.k_g]
p = {c = "g"}

[keys.k_h]
p = {c = "h"}

[keys.k_j]
p = {c = "j"}

[keys.k_k]
p = {c = "k"}

[keys.k_l]
p = {c = "l"}

[keys.k_semicolon]
p = {c = ";"}

[keys.k_apostrophe]
p = {c = "'"}

[keys.k_enter]
p = {ks = 0xff0a}

[keys.k_left_shift]
p = {ks = 0xffe1}

[keys.k_z]
p = {c = "z"}

[keys.k_x]
p = {c = "x"}

[keys.k_c]
p = {c = "c"}

[keys.k_v]
p = {c = "v"}

[keys.k_b]
p = {c = "b"}

[keys.k_n]
p = {c = "n"}

[keys.k_m]
p = {c = "m"}

[keys.k_comma]
p = {c = ","}

[keys.k_dot]
p = {c = "."}

[keys.k_slash]
p = {c = "/"}

[keys.k_right_shift]
p = {ks = 0xffe2}

[keys.k_left_ctrl]
p = {ks = 0xffe3}

[keys.k_left_super]
p = {ks = 0xffeb}

[keys.k_left_alt]
p = {ks = 0xffe9}

[keys.k_space]
p = {c = " "}

[keys.k_right_alt]
p = {ks = 0xffea}

[keys.k_right_super]
p = {ks = 0xffec}

[keys.k_print]
p = {ks = 0xff61}

[keys.k_right_ctrl]
p = {ks = 0xffe4}
"#;
