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
#k29 = "k_escape"
k29 = "k_caps_lock"
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
k59 = "k_right_super"
k60 = "k_right_ctrl"
"#;

pub const DEFAULT_KEY_SET_TOML: &str = r##"
name = "default"

[keys.k_grave_accent]
p = {c = "`", kc = 49}
[[keys.k_grave_accent.s]]
c = "~"
kc = -49

[keys.k_one]
p = {c = "1", kc = 10}
[[keys.k_one.s]]
c = "!"
kc = -10

[keys.k_two]
p = {c = "2", kc = 11}
[[keys.k_two.s]]
c = "@"
kc = -11

[keys.k_three]
p = {c = "3", kc = 12}
[[keys.k_three.s]]
c = "#"
kc = -12

[keys.k_four]
p = {c = "4", kc = 13}
[[keys.k_four.s]]
c = "$"
kc = -13

[keys.k_five]
p = {c = "5", kc = 14}
[[keys.k_five.s]]
c = "%"
kc = -14

[keys.k_six]
p = {c = "6", kc = 15}
[[keys.k_six.s]]
c = "^"
kc = -15

[keys.k_seven]
p = {c = "7", kc = 16}
[[keys.k_seven.s]]
c = "&"
kc = -16

[keys.k_eight]
p = {c = "8", kc = 17}
[[keys.k_eight.s]]
c = "*"
kc = -17

[keys.k_nine]
p = {c = "9", kc = 18}
[[keys.k_nine.s]]
c = "("
kc = -18

[keys.k_zero]
p = {c = "0", kc = 19}
[[keys.k_zero.s]]
c = ")"
kc = -19

[keys.k_hyphen]
p = {c = "-", kc = 20}
[[keys.k_hyphen.s]]
c = "_"
kc = -20

[keys.k_equals]
p = {c = "=", kc = 21}
[[keys.k_equals.s]]
c = "+"
kc = -21

[keys.k_backspace]
p = {ks = 0xff08, s = "Backspace", kc = 22}

[keys.k_tab]
p = {ks = 0xff09, kc = 23}

[keys.k_q]
p = {c = "q", kc = 24}
[[keys.k_q.s]]
c = "Q"
kc = -24

[keys.k_w]
p = {c = "w", kc = 25}
[[keys.k_w.s]]
c = "W"
kc = -25

[keys.k_e]
p = {c = "e", kc = 26}
[[keys.k_e.s]]
c = "E"
kc = -26

[keys.k_r]
p = {c = "r", kc = 27}
[[keys.k_r.s]]
c = "R"
kc = -27

[keys.k_t]
p = {c = "t", kc = 28}
[[keys.k_t.s]]
c = "T"
kc = -28

[keys.k_y]
p = {c = "y", kc = 29}
[[keys.k_y.s]]
c = "Y"
kc = -29

[keys.k_u]
p = {c = "u", kc = 30}
[[keys.k_u.s]]
c = "U"
kc = -30

[keys.k_i]
p = {c = "i", kc = 31}
[[keys.k_i.s]]
c = "I"
kc = -31

[keys.k_o]
p = {c = "o", kc = 32}
[[keys.k_o.s]]
c = "O"
kc = -32

[keys.k_p]
p = {c = "p", kc = 33}
[[keys.k_p.s]]
c = "P"
kc = -33

[keys.k_open_bracket]
p = {c = "[", kc = 34}
[[keys.k_open_bracket.s]]
c = "{"
kc = -34

[keys.k_close_bracket]
p = {c = "]", kc = 35}
[[keys.k_close_bracket.s]]
c = "}"
kc = -35

[keys.k_backslash]
p = {c = "\\", kc = 51}
[[keys.k_backslash.s]]
c = "|"
kc = -51

[keys.k_caps_lock]
p = {ks = 0xffe5, s = "CapsLock", kc = 66}

[keys.k_escape]
p = {ks = 0xff1b, kc = 9}

[keys.k_a]
p = {c = "a", kc = 38}
[[keys.k_a.s]]
c = "A"
kc = -38

[keys.k_s]
p = {c = "s", kc = 39}
[[keys.k_s.s]]
c = "S"
kc = -39

[keys.k_d]
p = {c = "d", kc = 40}
[[keys.k_d.s]]
c = "D"
kc = -40

[keys.k_f]
p = {c = "f", kc = 41}
[[keys.k_f.s]]
c = "F"
kc = -41

[keys.k_g]
p = {c = "g", kc = 42}
[[keys.k_g.s]]
c = "G"
kc = -42

[keys.k_h]
p = {c = "h", kc = 43}
[[keys.k_h.s]]
c = "H"
kc = -43

[keys.k_j]
p = {c = "j", kc = 44}
[[keys.k_j.s]]
c = "J"
kc = -44

[keys.k_k]
p = {c = "k", kc = 45}
[[keys.k_k.s]]
c = "K"
kc = -45

[keys.k_l]
p = {c = "l", kc = 46}
[[keys.k_l.s]]
c = "L"
kc = -46

[keys.k_semicolon]
p = {c = ";", kc = 47}
[[keys.k_semicolon.s]]
c = ":"
kc = -47

[keys.k_apostrophe]
p = {c = "'", kc = 48}
[[keys.k_apostrophe.s]]
c = "\""
kc = -48

[keys.k_enter]
p = {ks = 0xff0d, s = "Enter", kc = 36}
#p = {ks = 0xff0a}

[keys.k_left_shift]
p = {ks = 0xffe1, s = "Shift", kc = 50}

[keys.k_z]
p = {c = "z", kc = 52}
[[keys.k_z.s]]
c = "Z"
kc = -52

[keys.k_x]
p = {c = "x", kc = 53}
[[keys.k_x.s]]
c = "X"
kc = -53

[keys.k_c]
p = {c = "c", kc = 54}
[[keys.k_c.s]]
c = "C"
kc = -54

[keys.k_v]
p = {c = "v", kc = 55}
[[keys.k_v.s]]
c = "V"
kc = -55

[keys.k_b]
p = {c = "b", kc = 56}
[[keys.k_b.s]]
c = "B"
kc = -56

[keys.k_n]
p = {c = "n", kc = 57}
[[keys.k_n.s]]
c = "N"
kc = -57

[keys.k_m]
p = {c = "m", kc = 58}
[[keys.k_m.s]]
c = "M"
kc = -58

[keys.k_comma]
p = {c = ",", kc = 59}
[[keys.k_comma.s]]
c = "<"
kc = -59

[keys.k_dot]
p = {c = ".", kc = 60}
[[keys.k_dot.s]]
c = ">"
kc = -60

[keys.k_slash]
p = {c = "/", kc = 61}
[[keys.k_slash.s]]
c = "?"
kc = -61

[keys.k_right_shift]
p = {ks = 0xffe2, s = "Shift", kc = 62}

[keys.k_left_ctrl]
p = {ks = 0xffe3, s = "Ctrl", kc = 37}

[keys.k_left_super]
p = {ks = 0xffeb, s = "Super", kc = 133}

[keys.k_left_alt]
p = {ks = 0xffe9, s = "Alt", kc = 64}

[keys.k_space]
p = {c = " ", s = "Space", kc = 65}

[keys.k_right_alt]
p = {ks = 0xffea, s = "Alt", kc = 108}

[keys.k_right_super]
p = {ks = 0xffec, s = "Super", kc = 134}

[keys.k_print]
p = {ks = 0xff61, s = "Print", kc = 218}

[keys.k_right_ctrl]
p = {ks = 0xffe4, s = "Ctrl", kc = 105}
"##;
