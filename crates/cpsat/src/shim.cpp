// Protobuf in, protobuf out.
//
// The entire C++ surface of this crate is these few functions. Models are
// built in Rust as prost types, serialised, and parsed here; responses make the
// same trip back. Nothing crosses the boundary but bytes, so there are no
// shared C++ types, no lifetimes to reason about, and no ODR hazard beyond
// OR-Tools' own.

#include <cstdlib>
#include <cstring>
#include <string>

#include <ortools/sat/cp_model.h>
#include <ortools/sat/cp_model_checker.h>
#include <ortools/sat/cp_model_solver.h>

namespace sat = operations_research::sat;

namespace {

// Copy a byte range onto the malloc heap so Rust can take ownership and free it
// with cpsat_free. Returns nullptr for empty payloads.
unsigned char* to_owned(const void* data, size_t len, size_t* out_len) {
  *out_len = len;
  if (len == 0) return nullptr;
  unsigned char* buf = static_cast<unsigned char*>(std::malloc(len));
  std::memcpy(buf, data, len);
  return buf;
}

unsigned char* serialize(const google::protobuf::MessageLite& msg, size_t* out_len) {
  const std::string bytes = msg.SerializeAsString();
  return to_owned(bytes.data(), bytes.size(), out_len);
}

}  // namespace

extern "C" {

// Solve `model` under `params`. Both are serialised protos; `params` may be
// null. Returns a serialised CpSolverResponse, or null if either input fails to
// parse. Caller owns the result and must release it with cpsat_free.
unsigned char* cpsat_solve(const unsigned char* model_buf, size_t model_len,
                           const unsigned char* params_buf, size_t params_len,
                           size_t* out_len) {
  sat::CpModelProto model;
  if (!model.ParseFromArray(model_buf, static_cast<int>(model_len))) {
    *out_len = 0;
    return nullptr;
  }

  sat::SatParameters params;
  if (params_buf != nullptr &&
      !params.ParseFromArray(params_buf, static_cast<int>(params_len))) {
    *out_len = 0;
    return nullptr;
  }

  const sat::CpSolverResponse response = sat::SolveWithParameters(model, params);
  return serialize(response, out_len);
}

// Human-readable model statistics, as a NUL-terminated string owned by the
// caller. Release with cpsat_free_string.
char* cpsat_model_stats(const unsigned char* model_buf, size_t model_len) {
  sat::CpModelProto model;
  if (!model.ParseFromArray(model_buf, static_cast<int>(model_len))) return nullptr;
  const std::string stats = sat::CpModelStats(model);
  char* out = static_cast<char*>(std::malloc(stats.size() + 1));
  std::memcpy(out, stats.c_str(), stats.size() + 1);
  return out;
}

// Empty string means the model is valid; otherwise the validation error.
char* cpsat_validate(const unsigned char* model_buf, size_t model_len) {
  sat::CpModelProto model;
  if (!model.ParseFromArray(model_buf, static_cast<int>(model_len))) return nullptr;
  const std::string err = sat::ValidateCpModel(model);
  char* out = static_cast<char*>(std::malloc(err.size() + 1));
  std::memcpy(out, err.c_str(), err.size() + 1);
  return out;
}

void cpsat_free(unsigned char* buf) { std::free(buf); }
void cpsat_free_string(char* s) { std::free(s); }

}  // extern "C"
