; SSE2 scalar FP arithmetic — issue #291
; Tests fadd/fsub/fmul/fdiv for both double and float.
define double @fadd_double(double %a, double %b) {
entry:
  %r = fadd double %a, %b
  ret double %r
}
define double @fsub_double(double %a, double %b) {
entry:
  %r = fsub double %a, %b
  ret double %r
}
define double @fmul_double(double %a, double %b) {
entry:
  %r = fmul double %a, %b
  ret double %r
}
define double @fdiv_double(double %a, double %b) {
entry:
  %r = fdiv double %a, %b
  ret double %r
}
define float @fadd_float(float %a, float %b) {
entry:
  %r = fadd float %a, %b
  ret float %r
}
define float @fmul_float(float %a, float %b) {
entry:
  %r = fmul float %a, %b
  ret float %r
}
