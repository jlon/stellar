import { Component, Input, Output, EventEmitter, OnInit } from '@angular/core';

@Component({
  standalone: false,
  selector: 'ngx-nested-link-render',
  template: `
    <a href="javascript:void(0)" (click)="onClick()" class="text-primary">{{ renderValue }}</a>
  `,
  styles: [`
    a {
      cursor: pointer;
    }
  `]
})
export class NestedLinkRenderComponent implements OnInit {
  @Input() value: string | number;
  @Input() rowData: any;
  @Output() save: EventEmitter<any> = new EventEmitter();
  
  renderValue: string;
  
  ngOnInit() {
    this.renderValue = String(this.value);
  }
  
  onClick() {
    this.save.emit(this.rowData);
  }
}
