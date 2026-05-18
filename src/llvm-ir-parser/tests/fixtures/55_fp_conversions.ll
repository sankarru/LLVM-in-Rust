; FP conversion instructions — issue #291
; Tests sitofp, fptosi, fptrunc, fpext.
define double @sitofp_i64(i64 %n) {
entry:
  %r = sitofp i64 %n to double
  ret double %r
}
define i64 @fptosi_f64(double %x) {
entry:
  %r = fptosi double %x to i64
  ret i64 %r
}
define float @fptrunc_f64_to_f32(double %x) {
entry:
  %r = fptrunc double %x to float
  ret float %r
}
define double @fpext_f32_to_f64(float %x) {
entry:
  %r = fpext float %x to double
  ret double %r
}
