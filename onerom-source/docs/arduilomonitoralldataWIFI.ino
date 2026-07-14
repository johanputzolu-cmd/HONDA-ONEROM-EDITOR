
#include <SoftwareSerial.h>

SoftwareSerial espSerial(10, 11); // RX Arduino vers TX ESP, TX Arduino vers RX ESP

// === Paramètres globaux ===
int Injectors_Size = 240;
int mBarMin = -70;
int mBarMax = 1790;
int TempMin = -40;
int TempMax = 140;

const byte O2Input = 0;
const byte MapValue = 0;
const byte UseCelcius = 1;
const byte UseKMH = 1;
const byte O2Type = 0;
const double WBConversion[4] = {0, 0.71, 5, 1.3 };
const byte Tranny[4] = {70, 103, 142, 184};

bool IsAvailable = false;
byte Datalog_Bytes[52];
unsigned long last_datalog_time = 0;
unsigned long current_time;
const int Timeout = 40;

void setup() {
   Serial.begin(115200);     // Baudrate plus rapide
  Serial1.begin(38400);
  espSerial.begin(38400);     // liaison avec ESP8266

  Serial1.setTimeout(80);
}

unsigned long lastSendTime = 0;
const unsigned long sendInterval = 50; // ms

void loop() {
  current_time = millis();
  DataloggingThread();
  
  // Envoi formaté au ESP8266 toutes les 100 ms
  static unsigned long lastSend = 0;
  if (millis() - lastSend > 100) {
    lastSend = millis();
    sendDataToESP();
  }



bool MILActive = GetMIL();
  
  Serial.print("MIL Status: ");
Serial.println(MILActive ? "ON" : "OFF");





    // Envoi des valeurs au PC en format texte simple
Serial.print("RPM:"); Serial.println(GetRpm());
Serial.print("ECT:"); Serial.println(GetEct());
Serial.print("IAT:"); Serial.println(GetIat());
Serial.print("TPS:"); Serial.println(GetTps());
 Serial.print("Boost: "); Serial.print(GetBoost()); Serial.println(" PSI");
    Serial.print("MAP: "); Serial.print(GetMap()); Serial.println(" mbar");
    Serial.print("AFR: "); Serial.println(GetO2());
    Serial.print("Lambda: "); Serial.println(GetLambda(), 3);
    Serial.print("Battery: "); Serial.print(GetBattery()); Serial.println(" V");
    Serial.print("VSS: "); Serial.print(GetVssKMH()); Serial.println(" km/h");
    Serial.print("Gear: "); Serial.println(GetGear());
    Serial.print("Injection Duration: "); Serial.print(GetInjDuration()); Serial.println(" ms");
    Serial.print("Injector Duty: "); Serial.print(GetInjectorDuty()); Serial.println(" %");
    Serial.print("Ignition Advance: "); Serial.print(GetIgn()); Serial.println(" °");
    Serial.print("TPS Voltage: "); Serial.print(GetTPSVolt()); Serial.println(" V");
    Serial.print("MAP Voltage: "); Serial.print(GetMapVolt()); Serial.println(" V");

Serial.print("MIL: "); Serial.println(GetMIL() ? "1" : "0");
   Serial.print("FLR: "); Serial.println(GetOutputFTL() ? "ON" : "OFF");
    Serial.print("FuelCut1: "); Serial.println(GetFuelCut1() ? "YES" : "NO");
    Serial.print("FuelCut2: "); Serial.println(GetFuelCut2() ? "YES" : "NO");
    Serial.print("IgnCut: "); Serial.println(GetIgnCut() ? "YES" : "NO");
    Serial.print("LeanProtect: "); Serial.println(GetLeanProtect() ? "ON" : "OFF");

    Serial.print("FanCtrl: "); Serial.println(GetOutputFanCtrl() ? "ON" : "OFF");
    Serial.print("BoostCut: "); Serial.println(GetOutputBoostCut() ? "YES" : "NO");
    Serial.print("BST Out: "); Serial.println(GetOutputBST() ? "ON" : "OFF");
    Serial.print("Antilag: "); Serial.println(GetOutputAntilag() ? "ON" : "OFF");
    Serial.print("EBC Out: "); Serial.println(GetOutputEBC() ? "ON" : "OFF");

    Serial.print("AC: "); Serial.println(GetAC() ? "ON" : "OFF");
    Serial.print("ATL Ctrl: "); Serial.println(GetAtlCtrl() ? "ON" : "OFF");
    Serial.print("VTS: "); Serial.println(GetVTS() ? "ON" : "OFF");
    Serial.print("VTP: "); Serial.println(GetVTP() ? "ON" : "OFF");
    Serial.print("O2 Heater: "); Serial.println(GetO2Heater() ? "ON" : "OFF");

    Serial.print("IAB: "); Serial.println(GetIAB() ? "ON" : "OFF");
    Serial.print("Purge: "); Serial.println(GetPurge() ? "ON" : "OFF");
    Serial.print("FuelPump: "); Serial.println(GetFuelPump() ? "ON" : "OFF");
    Serial.print("Gear IC: "); Serial.println(GetGEARIC());
    Serial.print("EBC Duty: "); Serial.print(GetEBCDuty()); Serial.println(" %");

    Serial.print("Input FTL: "); Serial.println(GetInputFTL() ? "YES" : "NO");
    Serial.print("Input FTS: "); Serial.println(GetInputFTS() ? "YES" : "NO");
    Serial.print("Input EBC: "); Serial.println(GetInputEBC() ? "YES" : "NO");
    Serial.print("Input BST: "); Serial.println(GetInputBST() ? "YES" : "NO");

    Serial.print("GPO1: "); Serial.println(GetOutputGPO1() ? "ON" : "OFF");
    Serial.print("GPO2: "); Serial.println(GetOutputGPO2() ? "ON" : "OFF");
    Serial.print("GPO3: "); Serial.println(GetOutputGPO3() ? "ON" : "OFF");
    Serial.print("BST Stage 2: "); Serial.println(GetOutputBSTStage2() ? "ON" : "OFF");
    Serial.print("BST Stage 3: "); Serial.println(GetOutputBSTStage3() ? "ON" : "OFF");
    Serial.print("BST Stage 4: "); Serial.println(GetOutputBSTStage4() ? "ON" : "OFF");

Serial.print("Consumption: "); Serial.println(GetInstantConsumption());
Serial.print("IACV:"); Serial.println(GetIACVDuty());

    Serial.println();  // ligne vide pour séparer les paquets

  
}



void DataloggingThread() {
  current_time = millis();
  if (!IsAvailable) {
    Connect();
  } else if (current_time - last_datalog_time > Timeout) {
    GetData();
    last_datalog_time = current_time;
  }
}

void Connect() {
  if (current_time - last_datalog_time > 500) {
    Serial1.write((byte)16);
    if (Serial1.available()) {
      if (Serial1.read() == 205) {
        IsAvailable = true;
      }
    }
    Serial1.flush();
    last_datalog_time = current_time;
  }
}

void sendDataToESP() {
  String json = "{";
  json += "\"RPM\":" + String(GetRpm()) + ",";
  json += "\"ECT\":" + String(GetEct(),1) + ",";
  json += "\"IAT\":" + String(GetIat(),1) + ",";
  json += "\"TPS\":" + String(GetTps()) + ",";
  json += "\"Boost\":" + String(GetBoost(),2) + ",";
  json += "\"MAP\":" + String(GetMap()) + ",";
  json += "\"AFR\":" + String(GetO2(),2) + ",";
  json += "\"Lambda\":" + String(GetLambda(),3) + ",";
  json += "\"Battery\":" + String(GetBattery(),2) + ",";
  json += "\"VSS\":" + String(GetVssKMH()) + ",";
  json += "\"Gear\":" + String(GetGear()) + ",";
  json += "\"InjectionDuration\":" + String(GetInjDuration(),2);
  json += "}";
  
  espSerial.println(json);
}



void GetData() {
  for (int n = 0; n < 52; n++) {
    if (Serial1.available()) {
      Datalog_Bytes[n] = Serial1.read();
    }
  }
  Serial1.flush();
  Serial1.write(" ");
}

// Toutes les fonctions Get...() de code 1 doivent être ajoutées ici sans modification
// Ex : GetEct(), GetIat(), GetBoost(), GetTPSVolt(), GetGear(), GetDuty(), GetO2(), etc.
//Datalogging values that can be called.
long Long2Bytes(const byte ThisByte1, const byte ThisByte2) {
  return ((long) ThisByte2 * 256) + (long) ThisByte1;
}

float GetTemperature(const byte ThisByte) {
  float ThisTemp = (float) ThisByte / 51;
  ThisTemp = (0.1423 * pow(ThisTemp, 6)) - (2.4938 * pow(ThisTemp, 5))  + (17.837 * pow(ThisTemp, 4)) - (68.698 * pow(ThisTemp, 3)) + (154.69 * pow(ThisTemp, 2)) - (232.75 * ThisTemp) + 284.24;
  ThisTemp = ((ThisTemp - 25) * 5) / 9;

  return ThisTemp;
}

double GetVolt(const byte ThisByte) {
  return (double) ThisByte * 0.0196078438311815;
}

float GetDuration(const int ThisInt) {
  return ((float) ThisInt * 3.20000004768372) / 1000.0;
}

byte GetActivated(byte ThisByte, const int ThisPos, const bool Reversed) {
  int bitArray[8];
  for (int i = 0; i < 8; ++i ) {
    bitArray[i] = ThisByte & 1;
    ThisByte = ThisByte >> 1;
  }

  if (Reversed)
    return bitArray[ThisPos] ? (byte) 0 : (byte) 1;
  return bitArray[ThisPos] ? (byte) 1 : (byte) 0;
}

float GetInstantConsumption() {
  if (GetVssKMH() == 0) return 0;
  //float hundredkm = ((60 / GetVssKMH()) * 100) / 60;                      //minutes needed to travel 100km (OLD)
  float hundredkm = 6000 / GetVssKMH();                                     //minutes needed to travel 100km
  float fuelc = (hundredkm * ((Injectors_Size / 100) * GetDuty())) / 1000;
  return constrain(fuelc * 4, 0.0, 50.0);                                   //Vary between 0-50 L/100km
}

float GetDuty() {
  return ((float) GetRpm() * (float) GetInjDurr()) / 1200;
}

int GetInjDurr() {
  //return (int) (Long2Bytes(Datalog_Bytes[17], Datalog_Bytes[18]) / 352);
  return GetDuration((int) Long2Bytes(Datalog_Bytes[17], Datalog_Bytes[18]));

}
float GetEct() {

  return constrain(GetTemperature(Datalog_Bytes[0]), TempMin, TempMax);

}

float GetIat() {
  return constrain(GetTemperature(Datalog_Bytes[1]), TempMin, TempMax);
}

double GetO2() {
  byte WBByte = 0;
  if (O2Input == 0) WBByte = Datalog_Bytes[2];
  if (O2Input == 1) WBByte = Datalog_Bytes[24];
  if (O2Input == 2) WBByte = Datalog_Bytes[44];
  if (O2Input == 3) WBByte = Datalog_Bytes[45];
  double RTND = 0.0;
  if (O2Type == 0) RTND = constrain((double) InterpolateWB(GetVolt(WBByte)) * 14.7, 10, 20);
  if (O2Type == 1) RTND = constrain((double) InterpolateWB(GetVolt(WBByte)), 0, 5);
  if (O2Type == 2) RTND = constrain((double) GetVolt(WBByte), 0, 16);

  //return RoundThis(1, RTND);
  return RTND;
}

double GetLambda() {
  // If O2Type is AFR, convert to lambda. If already lambda, keep as-is.
  if (O2Type == 0) {
    return GetO2() / 14.7;
  }
  if (O2Type == 1) {
    return GetO2();
  }
  return 0.0;
}

double InterpolateWB(double ThisDouble) {
  if (ThisDouble < WBConversion[0]) return WBConversion[1];
  if (ThisDouble > WBConversion[2]) return WBConversion[3];
  return (WBConversion[1] + (((ThisDouble - WBConversion[0]) * (WBConversion[3] - WBConversion[1])) / (WBConversion[2] - WBConversion[0])));
}

int GetMBar() {

  long Value = (long) Datalog_Bytes[4];

  long MapLow = (long) mBarMin + 32768;
  long MapHigh = (long) mBarMax + 32768;

  return (int) ((((Value * (MapHigh - MapLow)) / 255) + MapLow) - 32768);
}

int GetMap() {
  int mBar = GetMBar();
  if (MapValue == 0) return constrain(mBar, mBarMin, mBarMax);
  else if (MapValue == 1) {
    if (mBar <= 1013) return 0;
    else return constrain((int) (((float) mBar - 1013) * 0.01450377), 0, 40); //GetValuePSI(ThisInt);
  }
  else if (MapValue == 2) return constrain((int) round((double) mBar * 0.1), 0, 105); //GetValueKPa(ThisInt);
  else return 0;
}

float GetBoost() {
  int ThisInt = GetMBar();
  if (ThisInt <= 1013) return 0;
  else return (double) (((double) ThisInt - 1013) * 0.0145037695765495);
}

unsigned int GetTps() {

  return constrain((int) round(((double) Datalog_Bytes[5] - 25.0) / 2.04), 0, 100);

  //return constrain((0.4716  * Datalog_Bytes[5]) - 11.3184, 0, 100);
}

unsigned int GetRpm() {
  //return (int) (1875000/Long2Bytes(Datalog_Bytes[6], Datalog_Bytes[7]));  //unused


  int rpm = (int) (1851562 / Long2Bytes(Datalog_Bytes[6], Datalog_Bytes[7]));

  return constrain(rpm, 0, 11000);
}

bool GetIgnCut() {
  return (bool) GetActivated(Datalog_Bytes[8], 2, false);
}

bool GetFuelCut1() {
  return (bool) GetActivated(Datalog_Bytes[8], 4, false);
}

unsigned int GetVss() {

  if (UseKMH == 1) return (int) Datalog_Bytes[16];
  return (int) round((float) Datalog_Bytes[16] / 1.6f);

}

unsigned int GetVssKMH() {

  return (unsigned int) Datalog_Bytes[16];

}

double GetInjFV() {

  return (double) Long2Bytes(Datalog_Bytes[17], Datalog_Bytes[18]) / 4.0;

}

float GetIgn() {
  return constrain((0.25 * Datalog_Bytes[19]) - 6, -6, 60);
}

float GetBattery() {
  return constrain((26.0 * Datalog_Bytes[25]) / 270.0, 0, 18);
}

bool GetOutput2ndMap() {
  return (bool) GetActivated(Datalog_Bytes[39], 5, false);
}

int GetIACVDuty() {
  return constrain((int) (Long2Bytes(Datalog_Bytes[49], Datalog_Bytes[50]) / 327.68) - 100, -100, 100);
}

double GetMapVolt() {
  return constrain(GetVolt(Datalog_Bytes[4]), 0, 5);
}

double GetTPSVolt() {
  return constrain(GetVolt(Datalog_Bytes[5]), 0, 5);
}

bool GetOutputFanCtrl() {
  return (bool) GetActivated(Datalog_Bytes[39], 6, false);
}

bool GetVTS() {
  return (bool) GetActivated(Datalog_Bytes[23], 7, false);
}

bool GetVTP() {
  return (bool) GetActivated(Datalog_Bytes[21], 3, false);
}

double GetELDVolt() {
  return GetVolt(Datalog_Bytes[24]);
}

bool GetO2Heater() {
  return (bool) GetActivated(Datalog_Bytes[23], 6, false);
}

bool GetAC() {
  return (bool) GetActivated(Datalog_Bytes[22], 7, false);
}

bool GetAtlCtrl() {
  return (bool) GetActivated(Datalog_Bytes[22], 5, false);
}

unsigned int GetGear() {
  long VSS = (long) GetVssKMH();
  long FAKERPM = Long2Bytes(Datalog_Bytes[6], Datalog_Bytes[7]);
  if (VSS == 0 | GetRpm() == 0) return 0;
  long num = ((VSS * 256) * FAKERPM) / 65535;
  for (int i = 0; i < 4; i++) if (num < (long) Tranny[i]) return i + 1;
  return 5;
}

bool GetOutputBST() {
  return (bool) GetActivated(Datalog_Bytes[39], 7, false);
}

bool GetOutputFTL() {
  return (bool) GetActivated(Datalog_Bytes[39], 0, false);
}

bool GetOutputAntilag() {
  return (bool) GetActivated(Datalog_Bytes[39], 1, false);
}

bool GetOutputFTS() {
  return (bool) GetActivated(Datalog_Bytes[39], 2, false);
}

bool GetOutputEBC() {
  return (bool) GetActivated(Datalog_Bytes[39], 4, false);
}

bool GetOutputBoostCut() {
  return (bool) GetActivated(Datalog_Bytes[39], 3, false);
}

bool GetLeanProtect() {
  return (bool) GetActivated(Datalog_Bytes[43], 7, false);
}

bool GetParkN() {
  return (bool) GetActivated(Datalog_Bytes[21], 0, false);
}

bool GetBKSW() {
  return (bool) GetActivated(Datalog_Bytes[21], 1, false);
}

bool GetACC() {
  return (bool) GetActivated(Datalog_Bytes[21], 2, false);
}

bool GetStart() {
  return (bool) GetActivated(Datalog_Bytes[21], 4, false);
}

bool GetSCC() {
  return (bool) GetActivated(Datalog_Bytes[21], 5, false);
}

bool GetFuelCut2() {
  return (bool) GetActivated(Datalog_Bytes[8], 5, false);
}

bool GetPSP() {
  return (bool) GetActivated(Datalog_Bytes[21], 7, false);
}

bool GetFuelPump() {
  return (bool) GetActivated(Datalog_Bytes[22], 0, false);
}

bool GetIAB() {
  return (bool) GetActivated(Datalog_Bytes[22], 2, false);
}

bool GetPurge() {
  return (bool) GetActivated(Datalog_Bytes[22], 6, false);
}

bool GetOutputGPO1() {
  return (bool) GetActivated(Datalog_Bytes[43], 0, false);
}

bool GetOutputGPO2() {
  return (bool) GetActivated(Datalog_Bytes[43], 1, false);
}

bool GetOutputGPO3() {
  return (bool) GetActivated(Datalog_Bytes[43], 2, false);
}

bool GetOutputBSTStage2() {
  return (bool) GetActivated(Datalog_Bytes[43], 3, false);
}

bool GetOutputBSTStage3() {
  return (bool) GetActivated(Datalog_Bytes[43], 4, false);
}

bool GetOutputBSTStage4() {
  return (bool) GetActivated(Datalog_Bytes[43], 5, false);
}
bool GetInputFTL(){
  return (bool) GetActivated(Datalog_Bytes[38], 0, false);
}

bool GetInputFTS(){
  return (bool) GetActivated(Datalog_Bytes[38], 1, false);
}

bool GetVTSFeedBack(){
  return (bool) GetActivated(Datalog_Bytes[21], 6, false);
}

bool GetInputEBC(){
  return (bool) GetActivated(Datalog_Bytes[38], 2, false);
}

bool GetInputBST(){
  return (bool) GetActivated(Datalog_Bytes[38], 7, false);
}

bool GetSCCChecker(){
  return (bool) GetActivated(Datalog_Bytes[8], 1, false);
}

bool GetVTSM(){
  return (bool) GetActivated(Datalog_Bytes[8], 3, false);
}

bool GetPostFuel(){
  return (bool) GetActivated(Datalog_Bytes[8], 0, false);
}

bool GetInputEBCHi(){
  return (bool) GetActivated(Datalog_Bytes[38], 3, false);
}

bool GetInputGPO1(){
  return (bool) GetActivated(Datalog_Bytes[38], 4, false);
}

bool GetInputGPO2(){
  return (bool) GetActivated(Datalog_Bytes[38], 5, false);
}

bool GetInputGPO3(){
  return (bool) GetActivated(Datalog_Bytes[38], 6, false);
}

bool GetATShift1(){
  return (bool) GetActivated(Datalog_Bytes[8], 6, false);
}

bool GetATShift2(){
  return (bool) GetActivated(Datalog_Bytes[8], 7, false);
}

float GetInjectorDuty() {
  return (float) ((double) GetRpm() * (double) GetDuration(Long2Bytes(Datalog_Bytes[17], Datalog_Bytes[18])) / 1200.0);
}

bool GetMIL(){
  return (bool) GetActivated(Datalog_Bytes[23], 5, false);
}

double GetInjDuration(){
  return round(((double) GetDuration((int) Long2Bytes(Datalog_Bytes[17], Datalog_Bytes[18]))) * 100) / 100;
}

double GetECTFC(){
  return GetFC(Datalog_Bytes[26], 128);
}

long GetO2Short(){
  return (long) GetFC(Long2Bytes(Datalog_Bytes[27], Datalog_Bytes[28]), 32768);
}

long GetO2Long(){
  return (long) GetFC(Long2Bytes(Datalog_Bytes[29], Datalog_Bytes[30]), 32768);
}

long GetIATFC(){
  return (long) GetFC(Long2Bytes(Datalog_Bytes[31], Datalog_Bytes[32]), 32768);
}

double GetVEFC(){
  return GetFC(Datalog_Bytes[33], 128);
}

float GetIATIC(){
  return GetIC(Datalog_Bytes[34]);
}

float GetECTIC(){
  return GetIC(Datalog_Bytes[35]);
}

float GetGEARIC(){
  return GetIC(Datalog_Bytes[36]);
}

double GetEBCBaseDuty(){
  return GetEBC(Datalog_Bytes[40]);
}

double GetEBCDuty(){
  return GetEBC(Datalog_Bytes[41]);
}

float GetIC(byte ThisByte) {
  if ((int) ThisByte == 128)
    return 0.0f;
  if ((int) ThisByte < 128)
    return (float) (128 - (int) ThisByte) * -0.25f;
  return (float) ((int) ThisByte - 128) * 0.25f;
}

double GetFC(long ThisByte, long ThisLong) {
  double num = (double) ThisByte / (double) ThisLong;
  //if (CorrectionUnits == "multi")
    return round((num) * 100) / 100;
  //return round(num * 100.0 - 100.0);
}

double GetEBC(byte ThisByte) {
  double num = (double) ThisByte / 2.0;
  if (num > 100.0)
    num = 100.0;
  return round((num * 10)) / 10;
}


