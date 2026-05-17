; Hand-written LLVM IR representing bzip2's run-length encoding (RLE) inner loop.
; Multiple phi nodes with back-edge forward references: current byte, run count,
; write pointer, and read pointer all thread through loop-carried values.
;
; Mirrors the structure of bzip2's BZ2_bzCompress block-sorting RLE step.

define i32 @bzip2_rle(ptr %src, ptr %dst, i32 %len) {
entry:
  %len64    = sext i32 %len to i64
  %first    = load i8, ptr %src
  br label %loop

loop:
  %ri      = phi i64 [ 0,       %entry  ], [ %ri_next,   %advance ]
  %wi      = phi i64 [ 0,       %entry  ], [ %wi_next,   %advance ]
  %run_len = phi i32 [ 0,       %entry  ], [ %run_next,  %advance ]
  %prev    = phi i8  [ %first,  %entry  ], [ %cur,       %advance ]
  %in_rng  = icmp ult i64 %ri, %len64
  br i1 %in_rng, label %read, label %flush

read:
  %rp  = getelementptr i8, ptr %src, i64 %ri
  %cur = load i8, ptr %rp
  %same = icmp eq i8 %cur, %prev
  br i1 %same, label %same_byte, label %diff_byte

same_byte:
  %run_inc = add i32 %run_len, 1
  %max_run = icmp eq i32 %run_inc, 255
  br i1 %max_run, label %emit, label %advance_same

advance_same:
  %ri_s  = add i64 %ri, 1
  %wi_s  = phi i64 [ %wi, %same_byte ]
  br label %advance_done

diff_byte:
  br label %emit

emit:
  %wp_run = getelementptr i8, ptr %dst, i64 %wi
  store i8 %prev, ptr %wp_run
  %wi_e1  = add i64 %wi, 1
  %rl8    = trunc i32 %run_len to i8
  %wp_cnt = getelementptr i8, ptr %dst, i64 %wi_e1
  store i8 %rl8, ptr %wp_cnt
  %wi_e2  = add i64 %wi_e1, 1
  %ri_e   = add i64 %ri, 1
  br label %advance_emit

advance_emit:
  %ri_ae = phi i64 [ %ri_e, %emit ]
  %wi_ae = phi i64 [ %wi_e2, %emit ]
  br label %advance_done

advance_done:
  %ri_ad   = phi i64 [ %ri_s, %advance_same ], [ %ri_ae, %advance_emit ]
  %wi_ad   = phi i64 [ %wi_s, %advance_same ], [ %wi_ae, %advance_emit ]
  %run_ad  = phi i32 [ %run_inc, %advance_same ], [ 0, %advance_emit ]
  %cur_ad  = phi i8  [ %cur, %advance_same ],    [ %cur, %advance_emit ]
  br label %advance

advance:
  %ri_next   = phi i64 [ %ri_ad, %advance_done ]
  %wi_next   = phi i64 [ %wi_ad, %advance_done ]
  %run_next  = phi i32 [ %run_ad, %advance_done ]
  %cur       = phi i8  [ %cur_ad, %advance_done ]
  br label %loop

flush:
  %wp_f  = getelementptr i8, ptr %dst, i64 %wi
  store i8 %prev, ptr %wp_f
  %wi_f1 = add i64 %wi, 1
  %rl8f  = trunc i32 %run_len to i8
  %wp_fc = getelementptr i8, ptr %dst, i64 %wi_f1
  store i8 %rl8f, ptr %wp_fc
  %wi_f2 = add i64 %wi_f1, 1
  %out_len = trunc i64 %wi_f2 to i32
  ret i32 %out_len
}
