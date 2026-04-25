// SPDX-License-Identifier: GPL-3.0-only
// Golden test vector generator for knx-rs.
// Compiles against knx-openknx source to produce JSON fixtures.
//
// Build: cd test-vectors && make

#include <cstdio>
#include <cstdint>
#include <cstring>
#include <cmath>

#include "dpt.h"
#include "dptconvert.h"
#include "knx_value.h"
#include "cemi_frame.h"

void print(const char*) {}
void print(unsigned int, int = 10) {}
void print(int, int = 10) {}
void println(const char* s = "") {}
void println(int, int = 10) {}
void printHex(const char*, const uint8_t*, int, bool = false) {}

static void print_hex(FILE* f, const uint8_t* data, int len) {
    fprintf(f, "[");
    for (int i = 0; i < len; i++) {
        if (i > 0) fprintf(f, ", ");
        fprintf(f, "%d", data[i]);
    }
    fprintf(f, "]");
}

// ── DPT vectors ──────────────────────────────────────────────

struct DptTestCase { int main_group; int sub_group; double value; };

static const DptTestCase dpt_cases[] = {
    {1, 1, 0}, {1, 1, 1}, {1, 2, 0}, {1, 2, 1},
    {5, 1, 0}, {5, 1, 50}, {5, 1, 100},
    {5, 3, 0}, {5, 3, 180}, {5, 3, 360},
    {5, 10, 0}, {5, 10, 42}, {5, 10, 255},
    {6, 1, 0}, {6, 1, -128}, {6, 1, 127},
    {7, 1, 0}, {7, 1, 1000}, {7, 1, 65535},
    {8, 1, 0}, {8, 1, -500}, {8, 1, 32767},
    {9, 1, 0}, {9, 1, 21.5}, {9, 1, -10.0}, {9, 1, -30.0},
    {9, 1, 670760.96}, {9, 4, 0}, {9, 4, 500.0}, {9, 4, 10000.0},
    {12, 1, 0}, {12, 1, 100000}, {12, 1, 4294967295.0},
    {13, 1, 0}, {13, 1, -100000}, {13, 1, 2147483647},
    {14, 56, 0}, {14, 56, 1234.5}, {14, 56, -273.15},
    {14, 68, 21.5}, {14, 0, 9.81},
};

static void generate_dpt_vectors(const char* path) {
    FILE* f = fopen(path, "w");
    fprintf(f, "[\n");
    int count = sizeof(dpt_cases) / sizeof(dpt_cases[0]);
    for (int i = 0; i < count; i++) {
        const auto& tc = dpt_cases[i];
        Dpt dpt(tc.main_group, tc.sub_group);
        int data_len = dpt.dataLength();
        uint8_t payload[16] = {};
        KNXValue val(tc.value);
        int ok = KNX_Encode_Value(val, payload, data_len, dpt);
        if (ok) {
            KNXValue decoded(0.0);
            int ok2 = KNX_Decode_Value(payload, data_len, dpt, decoded);
            double decoded_val = ok2 ? (double)decoded : 0.0;
            fprintf(f, "  {\"main\": %d, \"sub\": %d, \"input\": %.10g, \"bytes\": ",
                    tc.main_group, tc.sub_group, tc.value);
            print_hex(f, payload, data_len);
            fprintf(f, ", \"decoded\": %.10g}", decoded_val);
        } else {
            fprintf(f, "  {\"main\": %d, \"sub\": %d, \"input\": %.10g, \"bytes\": [], \"error\": true}",
                    tc.main_group, tc.sub_group, tc.value);
        }
        if (i < count - 1) fprintf(f, ",");
        fprintf(f, "\n");
    }
    fprintf(f, "]\n");
    fclose(f);
    printf("Generated %s (%d vectors)\n", path, count);
}

// ── CEMI vectors ─────────────────────────────────────────────

struct CemiTestCase { const char* name; uint8_t data[32]; int len; };

static const CemiTestCase cemi_cases[] = {
    {"group_write_bool_1_0_1", {0x29, 0x00, 0xBC, 0xE0, 0x11, 0x01, 0x08, 0x01, 0x01, 0x00, 0x81}, 11},
    {"group_write_bool_0_0_1", {0x29, 0x00, 0xBC, 0xE0, 0x11, 0x01, 0x08, 0x01, 0x01, 0x00, 0x80}, 11},
    {"group_read_1_0_1",       {0x29, 0x00, 0xBC, 0xE0, 0x11, 0x01, 0x08, 0x01, 0x00, 0x00, 0x00}, 11},
    {"group_response_1_0_1",   {0x29, 0x00, 0xBC, 0xE0, 0x11, 0x01, 0x08, 0x01, 0x01, 0x00, 0x41}, 11},
    {"l_data_req",             {0x11, 0x00, 0xBC, 0xE0, 0x00, 0x00, 0x08, 0x01, 0x01, 0x00, 0x81}, 11},
    {"system_priority",        {0x29, 0x00, 0xB0, 0xE0, 0x11, 0x01, 0x08, 0x01, 0x01, 0x00, 0x81}, 11},
    {"individual_dest",        {0x29, 0x00, 0xB0, 0x60, 0x11, 0x01, 0x11, 0x05, 0x01, 0x00, 0x81}, 11},
    {"dpt9_temp",              {0x29, 0x00, 0xBC, 0xE0, 0x11, 0x01, 0x08, 0x01, 0x03, 0x00, 0x80, 0x0C, 0x34}, 13},
};

static void generate_cemi_vectors(const char* path) {
    FILE* f = fopen(path, "w");
    fprintf(f, "[\n");
    int count = sizeof(cemi_cases) / sizeof(cemi_cases[0]);
    for (int i = 0; i < count; i++) {
        const auto& tc = cemi_cases[i];
        uint8_t buf[32];
        memcpy(buf, tc.data, tc.len);
        CemiFrame frame(buf, tc.len);
        fprintf(f, "  {\"name\": \"%s\", \"bytes\": ", tc.name);
        print_hex(f, tc.data, tc.len);
        fprintf(f, ", \"message_code\": %d", frame.messageCode());
        fprintf(f, ", \"frame_type\": %d", frame.frameType());
        fprintf(f, ", \"priority\": %d", frame.priority());
        fprintf(f, ", \"repetition\": %d", frame.repetition());
        fprintf(f, ", \"system_broadcast\": %d", frame.systemBroadcast());
        fprintf(f, ", \"ack\": %d", frame.ack());
        fprintf(f, ", \"confirm\": %d", frame.confirm());
        fprintf(f, ", \"address_type\": %d", frame.addressType());
        fprintf(f, ", \"hop_count\": %d", frame.hopCount());
        fprintf(f, ", \"source\": %d", frame.sourceAddress());
        fprintf(f, ", \"destination\": %d", frame.destinationAddress());
        fprintf(f, ", \"npdu_length\": %d", frame.npdu().octetCount());
        fprintf(f, ", \"total_length\": %d", frame.totalLenght());
        fprintf(f, "}");
        if (i < count - 1) fprintf(f, ",");
        fprintf(f, "\n");
    }
    fprintf(f, "]\n");
    fclose(f);
    printf("Generated %s (%d vectors)\n", path, count);
}

// ── CEMI setter vectors ──────────────────────────────────────

static void generate_cemi_setter_vectors(const char* path) {
    FILE* f = fopen(path, "w");
    fprintf(f, "[\n");

    // Start with a standard frame, modify each field, capture result
    uint8_t base[] = {0x29, 0x00, 0xBC, 0xE0, 0x11, 0x01, 0x08, 0x01, 0x01, 0x00, 0x81};
    int base_len = 11;

    struct SetterTest {
        const char* name;
        void (*apply)(CemiFrame&);
    };

    SetterTest tests[] = {
        {"set_priority_system", [](CemiFrame& f) { f.priority(SystemPriority); }},
        {"set_priority_urgent", [](CemiFrame& f) { f.priority(UrgentPriority); }},
        {"set_priority_normal", [](CemiFrame& f) { f.priority(NormalPriority); }},
        {"set_source_1_2_3", [](CemiFrame& f) { f.sourceAddress(0x1203); }},
        {"set_dest_2_0_5", [](CemiFrame& f) { f.destinationAddress(0x1005); }},
        {"set_hop_count_3", [](CemiFrame& f) { f.hopCount(3); }},
        {"set_hop_count_7", [](CemiFrame& f) { f.hopCount(7); }},
        {"set_ack_requested", [](CemiFrame& f) { f.ack(AckRequested); }},
        {"set_confirm_error", [](CemiFrame& f) { f.confirm(ConfirmError); }},
        {"set_repetition_repeated", [](CemiFrame& f) { f.repetition(NoRepitiion); }},
        {"set_frame_type_extended", [](CemiFrame& f) { f.frameType(ExtendedFrame); }},
        {"set_address_type_individual", [](CemiFrame& f) { f.addressType(IndividualAddress); }},
    };

    int count = sizeof(tests) / sizeof(tests[0]);
    for (int i = 0; i < count; i++) {
        uint8_t buf[32];
        memcpy(buf, base, base_len);
        CemiFrame frame(buf, base_len);
        tests[i].apply(frame);

        fprintf(f, "  {\"name\": \"%s\", \"base\": ", tests[i].name);
        print_hex(f, base, base_len);
        fprintf(f, ", \"result\": ");
        print_hex(f, buf, base_len);
        fprintf(f, ", \"priority\": %d", frame.priority());
        fprintf(f, ", \"source\": %d", frame.sourceAddress());
        fprintf(f, ", \"destination\": %d", frame.destinationAddress());
        fprintf(f, ", \"hop_count\": %d", frame.hopCount());
        fprintf(f, ", \"ack\": %d", frame.ack());
        fprintf(f, ", \"confirm\": %d", frame.confirm());
        fprintf(f, ", \"frame_type\": %d", frame.frameType());
        fprintf(f, ", \"address_type\": %d", frame.addressType());
        fprintf(f, "}");
        if (i < count - 1) fprintf(f, ",");
        fprintf(f, "\n");
    }

    fprintf(f, "]\n");
    fclose(f);
    printf("Generated %s (%d vectors)\n", path, count);
}

int main() {
    generate_dpt_vectors("dpt_vectors.json");
    generate_cemi_vectors("cemi_vectors.json");
    generate_cemi_setter_vectors("cemi_setter_vectors.json");
    return 0;
}
